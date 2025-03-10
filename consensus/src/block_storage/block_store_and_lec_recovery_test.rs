// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{
    block_storage::{block_store::BlockStore, BlockReader},
    persistent_liveness_storage::{LedgerRecoveryData, RecoveryData, RootMetadata},
    state_computer::ExecutionProxy,
    test_utils::{EmptyStorage, TreeInserter},
    util::mock_time_service::SimulatedTimeService,
};
use consensus_types::{block::Block, quorum_cert::QuorumCert};
use diem_config::config::NodeConfig;
use diem_crypto::{ed25519::Ed25519PrivateKey, Uniform};
use diem_types::validator_signer::ValidatorSigner;
use execution_correctness::{ExecutionCorrectness, ExecutionCorrectnessManager};
use executor_test_helpers::start_storage_service;
use executor_types::ExecutedTrees;
use futures::channel::mpsc;
use state_sync_v1::client::StateSyncClient;
use std::sync::Arc;
use storage_interface::DbReader;

fn get_initial_data_and_qc(db: &dyn DbReader) -> (RecoveryData, QuorumCert) {
    // find the block corresponding to storage latest ledger info
    let startup_info = db
        .get_startup_info()
        .expect("unable to read ledger info from storage")
        .expect("startup info is None");

    let ledger_info_with_sigs = startup_info.latest_ledger_info.clone();

    let qc = QuorumCert::certificate_for_genesis_from_ledger_info(
        ledger_info_with_sigs.ledger_info(),
        Block::make_genesis_block_from_ledger_info(ledger_info_with_sigs.ledger_info()).id(),
    );

    let ledger_recovery_data = LedgerRecoveryData::new(ledger_info_with_sigs);
    let frozen_root_hashes = startup_info
        .committed_tree_state
        .ledger_frozen_subtree_hashes
        .clone();
    let root_executed_trees = ExecutedTrees::from(startup_info.committed_tree_state);
    (
        RecoveryData::new(
            None,
            ledger_recovery_data,
            vec![],
            RootMetadata::new(
                root_executed_trees.txn_accumulator().num_leaves(),
                root_executed_trees.state_id(),
                frozen_root_hashes,
            ),
            vec![],
            None,
        )
        .unwrap(),
        qc,
    )
}

fn build_inserter(
    config: &NodeConfig,
    initial_data: RecoveryData,
    lec_client: Box<dyn ExecutionCorrectness + Send + Sync>,
) -> TreeInserter {
    let (coordinator_sender, _coordinator_receiver) = mpsc::unbounded();
    let client_commit_timeout_ms = config.state_sync.client_commit_timeout_ms;

    let state_computer = Arc::new(ExecutionProxy::new(
        lec_client,
        StateSyncClient::new(coordinator_sender, client_commit_timeout_ms),
    ));

    TreeInserter::new_with_store(
        ValidatorSigner::new(
            config.validator_network.as_ref().unwrap().peer_id(),
            Ed25519PrivateKey::generate_for_testing(),
        ),
        Arc::new(BlockStore::new(
            Arc::new(EmptyStorage::new()),
            initial_data,
            state_computer,
            10, // max pruned blocks in mem
            Arc::new(SimulatedTimeService::new()),
        )),
    )
}

#[test]
fn test_executor_restart() {
    // Start storage service
    let (config, _handle, db) = start_storage_service();
    let execution_correctness_manager = ExecutionCorrectnessManager::new(&config);

    let (initial_data, qc) = get_initial_data_and_qc(&*db);

    let mut inserter = build_inserter(
        &config,
        initial_data,
        execution_correctness_manager.client(),
    );

    let block_store = inserter.block_store();
    let genesis = block_store.ordered_root();
    let genesis_block_id = genesis.id();
    let genesis_block = block_store
        .get_block(genesis_block_id)
        .expect("genesis block must exist");

    //       ╭--> A1--> A2
    // Genesis--> B1
    let a1 = inserter.insert_block_with_qc(qc.clone(), &genesis_block, 1);
    let a2 = inserter.insert_block(&a1, 2, None);
    let b1 = inserter.insert_block_with_qc(qc, &genesis_block, 4);

    // Crash LEC.
    drop(execution_correctness_manager);

    // Restart LEC and make sure we can continue to append to the current tree.
    let _execution_correctness_manager = ExecutionCorrectnessManager::new(&config);

    //       ╭--> A1--> A2--> A3
    // Genesis--> B1--> B2
    //             ╰--> C1
    let _a3 = inserter.insert_block(&a2, 3, Some(genesis.block_info()));
    let _b2 = inserter.insert_block(&b1, 5, None);
    let _c1 = inserter.insert_block(&b1, 6, None);
}

#[test]
fn test_block_store_restart() {
    // Start storage service
    let (config, _handle, db) = start_storage_service();

    let execution_correctness_manager = ExecutionCorrectnessManager::new(&config);

    {
        let (initial_data, qc) = get_initial_data_and_qc(&*db);
        let mut inserter = build_inserter(
            &config,
            initial_data,
            execution_correctness_manager.client(),
        );

        let block_store = inserter.block_store();
        let genesis = block_store.ordered_root();
        let genesis_block_id = genesis.id();
        let genesis_block = block_store
            .get_block(genesis_block_id)
            .expect("genesis block must exist");

        //       ╭--> A1--> A2
        // Genesis--> B1
        let a1 = inserter.insert_block_with_qc(qc.clone(), &genesis_block, 1);
        let _a2 = inserter.insert_block(&a1, 2, None);
        let _b1 = inserter.insert_block_with_qc(qc, &genesis_block, 4);
    }

    // Restart block_store
    {
        let (initial_data, qc) = get_initial_data_and_qc(&*db);
        let mut inserter = build_inserter(
            &config,
            initial_data,
            execution_correctness_manager.client(),
        );
        let block_store = inserter.block_store();
        let genesis = block_store.ordered_root();
        let genesis_block_id = genesis.id();
        let genesis_block = block_store
            .get_block(genesis_block_id)
            .expect("genesis block must exist");

        //       ╭--> A1--> A2
        // Genesis--> B1
        let a1 = inserter.insert_block_with_qc(qc.clone(), &genesis_block, 1);
        let a2 = inserter.insert_block(&a1, 2, None);
        let b1 = inserter.insert_block_with_qc(qc, &genesis_block, 4);

        //       ╭--> A1--> A2--> A3
        // Genesis--> B1--> B2
        //             ╰--> C1
        let _a3 = inserter.insert_block(&a2, 3, Some(genesis.block_info()));
        let _b2 = inserter.insert_block(&b1, 5, None);
        let _c1 = inserter.insert_block(&b1, 6, None);
    }
}
