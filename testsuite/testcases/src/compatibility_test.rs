// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{batch_update, generate_traffic};
use anyhow::bail;
use forge::{NetworkContext, NetworkTest, Result, SwarmExt, Test};
use tokio::time::Duration;

pub struct SimpleValidatorUpgrade;

impl Test for SimpleValidatorUpgrade {
    fn name(&self) -> &'static str {
        "compatibility::simple-validator-upgrade"
    }
}

impl NetworkTest for SimpleValidatorUpgrade {
    fn run<'t>(&self, ctx: &mut NetworkContext<'t>) -> Result<()> {
        // Get the different versions we're testing with
        let (old_version, new_version) = {
            let mut versions = ctx.swarm().versions().collect::<Vec<_>>();
            versions.sort();
            if versions.len() != 2 {
                bail!("exactly two different versions needed to run compat test");
            }

            (versions[0].clone(), versions[1].clone())
        };

        println!("testing upgrade from {} -> {}", old_version, new_version);

        // Split the swarm into 2 parts
        if ctx.swarm().validators().count() < 4 {
            bail!("compat test requires >= 4 validators");
        }
        let all_validators = ctx
            .swarm()
            .validators()
            .map(|v| v.peer_id())
            .collect::<Vec<_>>();
        let mut first_batch = all_validators.clone();
        let second_batch = first_batch.split_off(first_batch.len() / 2);
        let first_node = first_batch.pop().unwrap();
        let duration = Duration::from_secs(5);

        println!("1. Downgrade all validators to older version");
        // Ensure that all validators are running the older version of the software
        let validators_to_downgrade = ctx
            .swarm()
            .validators()
            .filter(|v| v.version() != old_version)
            .map(|v| v.peer_id())
            .collect::<Vec<_>>();
        batch_update(ctx, &validators_to_downgrade, &old_version)?;

        // Generate some traffic
        generate_traffic(ctx, &all_validators, duration)?;

        // Update the first Validator
        println!("2. upgrading first Validator");
        batch_update(ctx, &[first_node], &new_version)?;
        generate_traffic(ctx, &[first_node], duration)?;

        // Update the rest of the first batch
        println!("3. upgrading rest of first batch");
        batch_update(ctx, &first_batch, &new_version)?;
        generate_traffic(ctx, &first_batch, duration)?;

        ctx.swarm().fork_check()?;

        // Update the second batch
        println!("4. upgrading second batch");
        batch_update(ctx, &second_batch, &new_version)?;
        generate_traffic(ctx, &second_batch, duration)?;

        println!("5. check swarm health");
        ctx.swarm().fork_check()?;

        Ok(())
    }
}
