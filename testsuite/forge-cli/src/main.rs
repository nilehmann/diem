// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0

use diem_sdk::{
    client::{BlockingClient, MethodRequest},
    move_types::account_address::AccountAddress,
    transaction_builder::Currency,
};
use forge::{forge_main, ForgeConfig, Options, Result, *};
use std::{num::NonZeroUsize, time::Duration};
use structopt::StructOpt;
use testcases::{
    compatibility_test::SimpleValidatorUpgrade, generate_traffic,
    performance_test::PerformanceBenchmark,
};

#[derive(StructOpt, Debug)]
struct Args {
    #[structopt(
        long,
        help = "If set, tries to connect to a local swarm instead of testnet"
    )]
    local_swarm: bool,

    // emit_tx options
    #[structopt(long, default_value = "15")]
    accounts_per_client: usize,
    #[structopt(long)]
    workers_per_ac: Option<usize>,
    #[structopt(
        long,
        help = "Time to run --emit-tx for in seconds",
        default_value = "60"
    )]
    duration: u64,

    #[structopt(flatten)]
    options: Options,

    #[structopt(subcommand)]
    ops_cmd: Option<OperatorCommand>,

    #[structopt(long, help = "Name of the EKS cluster")]
    cluster_name: String,
    #[structopt(
        long,
        help = "Override the helm repo used for k8s tests",
        default_value = "testnet-internal"
    )]
    helm_repo: String,
    #[structopt(
        long,
        help = "The image tag currently is used for validators",
        default_value = "devnet"
    )]
    image_tag: String,
    #[structopt(
        long,
        help = "Image tag for validator software to do backward compatibility test",
        default_value = "devnet"
    )]
    base_image_tag: String,
    #[structopt(long, help = "Specify a test suite to run")]
    suite: Option<String>,
}

#[derive(StructOpt, Debug)]
enum OperatorCommand {
    SetValidator(SetValidator),
    CleanUp(CleanUp),
    Resize(Resize),
}

#[derive(StructOpt, Debug)]
struct SetValidator {
    validator_name: String,
    #[structopt(long, help = "Override the image tag used for upgrade validators")]
    image_tag: String,
}

#[derive(StructOpt, Debug)]
struct CleanUp {
    #[structopt(long, help = "If set, uses k8s service account to auth with AWS")]
    auth_with_k8s_env: bool,
}

#[derive(StructOpt, Debug)]
struct Resize {
    #[structopt(long, default_value = "30")]
    num_validators: usize,
    #[structopt(
        long,
        help = "Override the image tag used for validators",
        default_value = "devnet"
    )]
    validator_image_tag: String,
    #[structopt(
        long,
        help = "Override the image tag used for testnet-specific components",
        default_value = "devnet"
    )]
    testnet_image_tag: String,
    #[structopt(
        long,
        help = "If set, performs validator healthcheck and assumes k8s DNS access"
    )]
    require_validator_healthcheck: bool,
    #[structopt(long, help = "If set, uses k8s service account to auth with AWS")]
    auth_with_k8s_env: bool,
}

fn main() -> Result<()> {
    let args = Args::from_args();

    if let Some(ops_cmd) = args.ops_cmd {
        match ops_cmd {
            OperatorCommand::SetValidator(set_validator) => {
                return set_validator_image_tag(
                    &set_validator.validator_name,
                    &set_validator.image_tag,
                    &args.helm_repo,
                )
            }
            OperatorCommand::Resize(resize) => {
                set_eks_nodegroup_size(
                    args.cluster_name,
                    resize.num_validators,
                    resize.auth_with_k8s_env,
                )?;
                uninstall_from_k8s_cluster()?;
                return clean_k8s_cluster(
                    args.helm_repo,
                    resize.num_validators,
                    resize.validator_image_tag,
                    resize.testnet_image_tag,
                    resize.require_validator_healthcheck,
                );
            }
            OperatorCommand::CleanUp(cleanup) => {
                uninstall_from_k8s_cluster()?;
                return set_eks_nodegroup_size(args.cluster_name, 0, cleanup.auth_with_k8s_env);
            }
        }
    }

    if args.local_swarm {
        forge_main(
            local_test_suite(),
            LocalFactory::from_workspace()?,
            &args.options,
        )
    } else {
        let mut test_suite = k8s_test_suite();
        if let Some(suite) = args.suite.as_ref() {
            test_suite = get_test_suite(suite);
        }
        forge_main(
            test_suite,
            K8sFactory::new(
                args.cluster_name,
                args.helm_repo,
                args.image_tag,
                args.base_image_tag,
            )
            .unwrap(),
            &args.options,
        )
    }
}

fn get_test_suite(suite_name: &str) -> ForgeConfig<'static> {
    match suite_name {
        "land_blocking_compat" => land_blocking_test_compat_suite(),
        "land_blocking" => land_blocking_test_suite(),
        _ => k8s_test_suite(),
    }
}

fn local_test_suite() -> ForgeConfig<'static> {
    ForgeConfig::default()
        .with_public_usage_tests(&[&FundAccount, &TransferCoins])
        .with_admin_tests(&[&GetMetadata])
        .with_network_tests(&[&RestartValidator, &EmitTransaction])
}

fn k8s_test_suite() -> ForgeConfig<'static> {
    ForgeConfig::default()
        .with_initial_validator_count(NonZeroUsize::new(30).unwrap())
        .with_public_usage_tests(&[&FundAccount, &TransferCoins])
        .with_admin_tests(&[&GetMetadata])
        .with_network_tests(&[&EmitTransaction, &SimpleValidatorUpgrade])
}

fn land_blocking_test_suite() -> ForgeConfig<'static> {
    ForgeConfig::default()
        .with_initial_validator_count(NonZeroUsize::new(30).unwrap())
        .with_network_tests(&[&PerformanceBenchmark])
}

fn land_blocking_test_compat_suite() -> ForgeConfig<'static> {
    ForgeConfig::default()
        .with_initial_validator_count(NonZeroUsize::new(30).unwrap())
        .with_network_tests(&[&PerformanceBenchmark, &SimpleValidatorUpgrade])
}

//TODO Make public test later
#[derive(Debug)]
struct GetMetadata;

impl Test for GetMetadata {
    fn name(&self) -> &'static str {
        "get_metadata"
    }
}

impl AdminTest for GetMetadata {
    fn run<'t>(&self, ctx: &mut AdminContext<'t>) -> Result<()> {
        let client = ctx.client();

        let metadata = client.get_metadata()?.into_inner();

        // get_metadata documentation states that the following fields will be present when no version
        // argument is provided
        metadata.script_hash_allow_list.unwrap();
        metadata.diem_version.unwrap();
        metadata.module_publishing_allowed.unwrap();
        metadata.dual_attestation_limit.unwrap();

        Ok(())
    }
}

pub fn check_account_balance(
    client: &BlockingClient,
    currency: Currency,
    account_address: AccountAddress,
    expected: u64,
) -> Result<()> {
    let account_view = client.get_account(account_address)?.into_inner().unwrap();
    let balance = account_view
        .balances
        .iter()
        .find(|b| b.currency == currency)
        .unwrap();
    assert_eq!(balance.amount, expected);

    Ok(())
}

#[derive(Debug)]
struct FundAccount;

impl Test for FundAccount {
    fn name(&self) -> &'static str {
        "fund_account"
    }
}

impl PublicUsageTest for FundAccount {
    fn run<'t>(&self, ctx: &mut PublicUsageContext<'t>) -> Result<()> {
        let client = ctx.client();

        let account = ctx.random_account();
        let amount = 1000;
        let currency = Currency::XUS;
        ctx.create_parent_vasp_account(account.authentication_key())?;
        ctx.fund(account.address(), amount)?;
        check_account_balance(&client, currency, account.address(), amount)?;

        Ok(())
    }
}

#[derive(Debug)]
struct TransferCoins;

impl Test for TransferCoins {
    fn name(&self) -> &'static str {
        "transfer_coins"
    }
}

impl PublicUsageTest for TransferCoins {
    fn run<'t>(&self, ctx: &mut PublicUsageContext<'t>) -> Result<()> {
        let mut account = ctx.random_account();
        let amount = 1000;
        let currency = Currency::XUS;
        let client = ctx.client();
        ctx.create_parent_vasp_account(account.authentication_key())?;
        ctx.fund(account.address(), amount)?;

        let mut payer = ctx.random_account();
        let payee = ctx.random_account();
        let create_payer = account.sign_with_transaction_builder(
            ctx.transaction_factory().create_child_vasp_account(
                currency,
                payer.authentication_key(),
                false,
                100,
            ),
        );
        let create_payee = account.sign_with_transaction_builder(
            ctx.transaction_factory().create_child_vasp_account(
                currency,
                payee.authentication_key(),
                false,
                0,
            ),
        );
        let batch = vec![
            MethodRequest::submit(&create_payer)?,
            MethodRequest::submit(&create_payee)?,
        ];
        client.batch(batch)?;
        client.wait_for_signed_transaction(&create_payer, None, None)?;
        client.wait_for_signed_transaction(&create_payee, None, None)?;
        check_account_balance(&client, currency, payer.address(), 100)?;

        ctx.transfer_coins(currency, &mut payer, payee.address(), 10)?;
        check_account_balance(&client, currency, payer.address(), 90)?;
        check_account_balance(&client, currency, payee.address(), 10)?;
        let account_view = client.get_account(payee.address())?.into_inner().unwrap();
        let balance = account_view
            .balances
            .iter()
            .find(|b| b.currency == currency)
            .unwrap();
        assert_eq!(balance.amount, 10);

        Ok(())
    }
}

#[derive(Debug)]
struct RestartValidator;

impl Test for RestartValidator {
    fn name(&self) -> &'static str {
        "restart_validator"
    }
}

impl NetworkTest for RestartValidator {
    fn run<'t>(&self, ctx: &mut NetworkContext<'t>) -> Result<()> {
        let node = ctx.swarm().validators_mut().next().unwrap();
        node.health_check().expect("node health check failed");
        node.stop()?;
        println!("Restarting node {}", node.peer_id());
        node.start()?;
        // wait node to recovery
        std::thread::sleep(Duration::from_millis(1000));
        node.health_check().expect("node health check failed");

        Ok(())
    }
}

#[derive(Debug)]
struct EmitTransaction;

impl Test for EmitTransaction {
    fn name(&self) -> &'static str {
        "emit_transaction"
    }
}

impl NetworkTest for EmitTransaction {
    fn run<'t>(&self, ctx: &mut NetworkContext<'t>) -> Result<()> {
        let duration = Duration::from_secs(10);
        let all_validators = ctx
            .swarm()
            .validators()
            .map(|v| v.peer_id())
            .collect::<Vec<_>>();
        let stats = generate_traffic(ctx, &all_validators, duration).unwrap();
        ctx.report
            .report_txn_stats(self.name().to_string(), stats, duration);
        ctx.report.print_report();

        Ok(())
    }
}
