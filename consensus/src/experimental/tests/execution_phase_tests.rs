// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{
    experimental::execution_phase::{ExecutionPhase, ExecutionRequest, ExecutionResponse},
    test_utils::{consensus_runtime, timed_block_on, RandomComputeResultStateComputer},
};
use consensus_types::block::{block_test_utils::certificate_for_genesis, Block};
use diem_crypto::HashValue;
use diem_types::validator_verifier::random_validator_verifier;
use futures::{
    channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender},
    SinkExt, StreamExt,
};
use std::sync::Arc;

pub fn prepare_execution_phase() -> (
    UnboundedSender<ExecutionRequest>,
    UnboundedReceiver<ExecutionResponse>,
    HashValue,
    ExecutionPhase,
) {
    let (in_channel_tx, in_channel_rx) = unbounded::<ExecutionRequest>();
    let (out_channel_tx, out_channel_rx) = unbounded::<ExecutionResponse>();

    let execution_proxy = Arc::new(RandomComputeResultStateComputer::new());

    let random_hash_value = execution_proxy.get_root_hash();

    let execution_phase = ExecutionPhase::new(in_channel_rx, out_channel_tx, execution_proxy);

    (
        in_channel_tx,
        out_channel_rx,
        random_hash_value,
        execution_phase,
    )
}

// unit tests
#[test]
fn test_execution_phase_process() {
    let mut runtime = consensus_runtime();

    let (_in_channel_tx, _out_channel_rx, random_hash_value, execution_phase) =
        prepare_execution_phase();

    let genesis_qc = certificate_for_genesis();
    let (signers, _validators) = random_validator_verifier(1, None, false);
    let block = Block::new_proposal(vec![], 1, 1, genesis_qc, &signers[0]);

    timed_block_on(&mut runtime, async move {
        let out_item = execution_phase
            .process(ExecutionRequest {
                blocks: vec![block],
            })
            .await;

        assert_eq!(
            out_item.inner.unwrap()[0].compute_result().root_hash(),
            random_hash_value
        );
    });
}

#[test]
fn test_execution_phase_happy_path() {
    let mut runtime = consensus_runtime();

    let (mut in_channel_tx, mut out_channel_rx, random_hash_value, execution_phase) =
        prepare_execution_phase();

    runtime.spawn(execution_phase.start());

    let genesis_qc = certificate_for_genesis();
    let (signers, _validators) = random_validator_verifier(1, None, false);
    let block = Block::new_proposal(vec![], 1, 1, genesis_qc, &signers[0]);

    timed_block_on(&mut runtime, async move {
        in_channel_tx
            .send(ExecutionRequest {
                blocks: vec![block],
            })
            .await
            .ok();

        let out_item = out_channel_rx.next().await.unwrap();

        assert_eq!(
            out_item.inner.unwrap()[0].compute_result().root_hash(),
            random_hash_value
        );
    });
}

// TODO: unhappy paths
