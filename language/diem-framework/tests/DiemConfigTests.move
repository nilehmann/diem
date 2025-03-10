#[test_only]
module DiemFramework::OnChainConfigTests {
    use DiemFramework::DiemConfig;
    use DiemFramework::Genesis;

    #[test(account = @0x1)]
    #[expected_failure(abort_code = 2)]
    fun init_before_genesis(account: signer) {
        DiemConfig::initialize(&account);
    }

    #[test(account = @0x2, tc = @TreasuryCompliance, dr = @DiemRoot)]
    #[expected_failure(abort_code = 1)]
    fun invalid_address_init(account: signer, tc: signer, dr: signer) {
        Genesis::setup(&dr, &tc);
        DiemConfig::initialize(&account);
    }

    #[test(tc = @TreasuryCompliance, dr = @DiemRoot)]
    #[expected_failure(abort_code = 261)]
    fun invalid_get(tc: signer, dr: signer) {
        Genesis::setup(&dr, &tc);
        DiemConfig::get<u64>();
    }

    #[test(account = @0x1, tc = @TreasuryCompliance, dr = @DiemRoot)]
    #[expected_failure(abort_code = 516)]
    fun invalid_set(account: signer, tc: signer, dr: signer) {
        Genesis::setup(&dr, &tc);
        DiemConfig::set_for_testing(&account, 0);
    }

    #[test(account = @0x1, tc = @TreasuryCompliance, dr = @DiemRoot)]
    #[expected_failure(abort_code = 2)]
    fun invalid_publish(account: signer, tc: signer, dr: signer) {
        Genesis::setup(&dr, &tc);
        DiemConfig::publish_new_config(&account, 0);
    }
}
