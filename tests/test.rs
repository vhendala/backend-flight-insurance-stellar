// test.rs

#![cfg(test)]

use flight_delay_insurance_contract::{FlightInsuranceContract, FlightInsuranceContractClient};

use soroban_sdk::{
    testutils::{Address as _, Ledger, LedgerInfo},
    token, Address, Env, IntoVal, String, // Correção: IntoVal importado
};

fn create_token_contract(env: &Env, admin: &Address) -> Address {
    env.register_stellar_asset_contract_v2(admin.clone()).address()
}

fn create_insurance_contract<'a>(env: &Env) -> FlightInsuranceContractClient<'a> {
    let contract_id = env.register(FlightInsuranceContract, ());
    FlightInsuranceContractClient::new(env, &contract_id)
}

#[test]
fn test_contract_initialization() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let token_addr = create_token_contract(&env, &admin);
    let contract = create_insurance_contract(&env);
    let initial_capital = 10_000_0000000i128;
    contract.initialize(&admin, &token_addr, &initial_capital);
    assert_eq!(contract.get_liquidity_pool(), initial_capital);
    assert!(contract.is_admin(&admin));
    assert_eq!(contract.get_total_policies(), 0);
}

#[test]
fn test_create_policy_success() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let customer = Address::generate(&env);
    let token_addr = create_token_contract(&env, &admin);
    let token_admin_client = token::StellarAssetClient::new(&env, &token_addr);
    let token_client = token::Client::new(&env, &token_addr);
    let contract = create_insurance_contract(&env);
    let initial_capital = 10_000_0000000i128;
    contract.initialize(&admin, &token_addr, &initial_capital);
    let premium_amount = 841_0000i128;
    token_admin_client.mint(&customer, &premium_amount);
    let flight_id = String::from_str(&env, "G32102");
    let flight_date = env.ledger().timestamp() + (24 * 60 * 60);
    let coverage_amount = 50_0000000i128;
    let policy_id = contract.create_policy(
        &customer,
        &flight_id,
        &flight_date,
        &premium_amount,
        &coverage_amount,
    );
    assert_eq!(policy_id, 1);
    let policy = contract.get_policy(&policy_id);
    assert_eq!(policy.customer, customer);
    assert_eq!(policy.flight_id, flight_id);
    assert_eq!(policy.premium_amount, premium_amount);
    assert_eq!(policy.coverage_amount, coverage_amount);
    assert!(!policy.resolved);
    assert!(!policy.paid_out);
    let expected_pool = initial_capital + premium_amount;
    assert_eq!(contract.get_liquidity_pool(), expected_pool);
    assert_eq!(token_client.balance(&customer), 0);
    assert_eq!(token_client.balance(&contract.address), premium_amount);
    let active = contract.get_active_policies();
    assert_eq!(active.len(), 1);
    assert_eq!(active.get(0).unwrap(), policy_id);
}

#[test]
#[should_panic(expected = "Insufficient liquidity pool")]
fn test_create_policy_insufficient_pool() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let customer = Address::generate(&env);
    let token_addr = create_token_contract(&env, &admin);
    let token_admin_client = token::StellarAssetClient::new(&env, &token_addr);
    let contract = create_insurance_contract(&env);
    let initial_capital = 10_0000000i128;
    contract.initialize(&admin, &token_addr, &initial_capital);
    let premium_amount = 8_0000000i128;
    let coverage_amount = 50_0000000i128;
    token_admin_client.mint(&customer, &premium_amount);
    let flight_id = String::from_str(&env, "G32102");
    let flight_date = env.ledger().timestamp() + (24 * 60 * 60);
    contract.create_policy(&customer, &flight_id, &flight_date, &premium_amount, &coverage_amount);
}

#[test]
fn test_resolve_policy_no_delay() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let customer = Address::generate(&env);
    let token_addr = create_token_contract(&env, &admin);
    let token_admin_client = token::StellarAssetClient::new(&env, &token_addr);
    let token_client = token::Client::new(&env, &token_addr);
    let contract = create_insurance_contract(&env);
    let initial_capital = 10_000_0000000i128;
    contract.initialize(&admin, &token_addr, &initial_capital);
    let premium_amount = 841_0000i128;
    let coverage_amount = 50_0000000i128;
    token_admin_client.mint(&customer, &premium_amount);
    let flight_id = String::from_str(&env, "G32102");
    let flight_date = env.ledger().timestamp() + (24 * 60 * 60);
    let policy_id = contract.create_policy(
        &customer, &flight_id, &flight_date, &premium_amount, &coverage_amount
    );
    env.ledger().with_mut(|li| {
        li.timestamp = flight_date + 3600;
    });
    let pool_before = contract.get_liquidity_pool();
    let customer_balance_before = token_client.balance(&customer);
    contract.resolve_policy(&policy_id, &false);
    let policy = contract.get_policy(&policy_id);
    assert!(policy.resolved);
    assert!(!policy.paid_out);
    assert_eq!(contract.get_liquidity_pool(), pool_before);
    assert_eq!(token_client.balance(&customer), customer_balance_before);
    let active = contract.get_active_policies();
    assert_eq!(active.len(), 0);
}

#[test]
fn test_resolve_policy_with_delay() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let customer = Address::generate(&env);
    let token_addr = create_token_contract(&env, &admin);
    let token_admin_client = token::StellarAssetClient::new(&env, &token_addr);
    let token_client = token::Client::new(&env, &token_addr);
    let contract = create_insurance_contract(&env);
    let initial_capital = 10_000_0000000i128;
    contract.initialize(&admin, &token_addr, &initial_capital);
    let premium_amount = 841_0000i128;
    let coverage_amount = 50_0000000i128;
    token_admin_client.mint(&customer, &premium_amount);
    token_admin_client.mint(&contract.address, &(initial_capital + premium_amount));
    let flight_id = String::from_str(&env, "G32102");
    let flight_date = env.ledger().timestamp() + (24 * 60 * 60);
    let policy_id = contract.create_policy(
        &customer, &flight_id, &flight_date, &premium_amount, &coverage_amount
    );
    env.ledger().with_mut(|li| {
        li.timestamp = flight_date + 3600;
    });
    let pool_before = contract.get_liquidity_pool();
    let customer_balance_before = token_client.balance(&customer);
    contract.resolve_policy(&policy_id, &true);
    let policy = contract.get_policy(&policy_id);
    assert!(policy.resolved);
    assert!(policy.paid_out);
    let expected_pool = pool_before - coverage_amount;
    assert_eq!(contract.get_liquidity_pool(), expected_pool);
    let expected_balance = customer_balance_before + coverage_amount;
    assert_eq!(token_client.balance(&customer), expected_balance);
}

#[test]
#[should_panic]
fn test_resolve_policy_not_admin() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let customer = Address::generate(&env);
    let impostor = Address::generate(&env);
    let token_addr = create_token_contract(&env, &admin);
    let token_admin_client = token::StellarAssetClient::new(&env, &token_addr);
    let contract = create_insurance_contract(&env);
    contract.initialize(&admin, &token_addr, &10_000_0000000i128);
    token_admin_client.mint(&customer, &1000);
    env.mock_all_auths();
    let policy_id = contract.create_policy(
        &customer, &String::from_str(&env, "F01"), &(env.ledger().timestamp() + 100), &100, &500
    );
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &impostor,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &contract.address,
            fn_name: "resolve_policy",
            args: (policy_id, false).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    contract.resolve_policy(&policy_id, &false);
}

#[test]
fn test_deposit_and_withdraw_pool() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let token_addr = create_token_contract(&env, &admin);
    let token_admin_client = token::StellarAssetClient::new(&env, &token_addr);
    let token_client = token::Client::new(&env, &token_addr);
    let contract = create_insurance_contract(&env);
    let initial_capital = 10_000_0000000i128;
    contract.initialize(&admin, &token_addr, &initial_capital);
    token_admin_client.mint(&contract.address, &initial_capital);
    let additional_deposit = 5_000_0000000i128;
    token_admin_client.mint(&admin, &additional_deposit);
    contract.deposit_to_pool(&additional_deposit);
    let expected_pool = initial_capital + additional_deposit;
    assert_eq!(contract.get_liquidity_pool(), expected_pool);
    let withdrawal = 2_000_0000000i128;
    let admin_balance_before = token_client.balance(&admin);
    contract.withdraw_from_pool(&withdrawal);
    let final_pool = expected_pool - withdrawal;
    assert_eq!(contract.get_liquidity_pool(), final_pool);
    let admin_balance_after = token_client.balance(&admin);
    assert_eq!(admin_balance_after, admin_balance_before + withdrawal);
}

#[test]
fn test_multiple_policies() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let customer1 = Address::generate(&env);
    let customer2 = Address::generate(&env);
    let token_addr = create_token_contract(&env, &admin);
    let token_admin_client = token::StellarAssetClient::new(&env, &token_addr);
    let contract = create_insurance_contract(&env);
    let initial_capital = 10_000_0000000i128;
    contract.initialize(&admin, &token_addr, &initial_capital);
    let premium_amount1 = 841_0000i128;
    let coverage_amount1 = 50_0000000i128;
    token_admin_client.mint(&customer1, &premium_amount1);
    let policy_id1 = contract.create_policy(
        &customer1, 
        &String::from_str(&env, "G32102"),
        &(env.ledger().timestamp() + 86400),
        &premium_amount1, 
        &coverage_amount1
    );
    let premium_amount2 = 1200_0000i128;
    let coverage_amount2 = 75_0000000i128;
    token_admin_client.mint(&customer2, &premium_amount2);
    let policy_id2 = contract.create_policy(
        &customer2,
        &String::from_str(&env, "LA4567"),
        &(env.ledger().timestamp() + 172800),
        &premium_amount2,
        &coverage_amount2
    );
    assert_eq!(policy_id1, 1);
    assert_eq!(policy_id2, 2);
    assert_eq!(contract.get_total_policies(), 2);
    let active = contract.get_active_policies();
    assert_eq!(active.len(), 2);
    let expected_pool = initial_capital + premium_amount1 + premium_amount2;
    assert_eq!(contract.get_liquidity_pool(), expected_pool);
}

#[test]
#[should_panic(expected = "Flight date must be in the future")]
fn test_create_policy_past_date() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let customer = Address::generate(&env);
    let token_addr = create_token_contract(&env, &admin);
    let token_admin_client = token::StellarAssetClient::new(&env, &token_addr);
    let contract = create_insurance_contract(&env);

    let initial_capital = 10_000_0000000i128;
    contract.initialize(&admin, &token_addr, &initial_capital);

    let premium_amount = 841_0000i128;
    let coverage_amount = 50_0000000i128;
    token_admin_client.mint(&customer, &premium_amount);

    // --- CORREÇÃO APLICADA AQUI ---
    // 1. Define um timestamp inicial para o ledger.
    env.ledger().with_mut(|li| {
        li.timestamp = 100_000;
    });

    let flight_id = String::from_str(&env, "G32102");
    // 2. Agora a subtração funciona sem overflow.
    let past_date = env.ledger().timestamp() - 3600; 

    // 3. A chamada abaixo agora vai falhar com a mensagem correta do contrato.
    contract.create_policy(&customer, &flight_id, &past_date, &premium_amount, &coverage_amount);
}

#[test]
#[should_panic(expected = "Policy already resolved")]
fn test_resolve_policy_twice() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let customer = Address::generate(&env);
    let token_addr = create_token_contract(&env, &admin);
    let token_admin_client = token::StellarAssetClient::new(&env, &token_addr);
    let contract = create_insurance_contract(&env);
    let initial_capital = 10_000_0000000i128;
    contract.initialize(&admin, &token_addr, &initial_capital);
    let premium_amount = 841_0000i128;
    let coverage_amount = 50_0000000i128;
    token_admin_client.mint(&customer, &premium_amount);
    token_admin_client.mint(&contract.address, &(initial_capital + premium_amount));
    let flight_id = String::from_str(&env, "G32102");
    let flight_date = env.ledger().timestamp() + (24 * 60 * 60);
    let policy_id = contract.create_policy(
        &customer, &flight_id, &flight_date, &premium_amount, &coverage_amount
    );
    env.ledger().with_mut(|li: &mut LedgerInfo| {
        li.timestamp = flight_date + 3600;
    });
    contract.resolve_policy(&policy_id, &false);
    contract.resolve_policy(&policy_id, &false);
}

#[test]
#[should_panic(expected = "Resolution deadline expired")]
fn test_resolve_policy_too_late() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let customer = Address::generate(&env);
    let token_addr = create_token_contract(&env, &admin);
    let token_admin_client = token::StellarAssetClient::new(&env, &token_addr);
    let contract = create_insurance_contract(&env);
    let initial_capital = 10_000_0000000i128;
    contract.initialize(&admin, &token_addr, &initial_capital);
    let premium_amount = 841_0000i128;
    let coverage_amount = 50_0000000i128;
    token_admin_client.mint(&customer, &premium_amount);
    let flight_id = String::from_str(&env, "G32102");
    let flight_date = env.ledger().timestamp() + (24 * 60 * 60);
    let policy_id = contract.create_policy(
        &customer, &flight_id, &flight_date, &premium_amount, &coverage_amount
    );
    env.ledger().with_mut(|li| {
        li.timestamp = flight_date + (25 * 60 * 60);
    });
    contract.resolve_policy(&policy_id, &false);
}

#[test]
#[should_panic(expected = "Withdrawal would compromise active policies coverage")]
fn test_withdraw_compromises_active_policies() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let customer = Address::generate(&env);
    let token_addr = create_token_contract(&env, &admin);
    let token_admin_client = token::StellarAssetClient::new(&env, &token_addr);
    let contract = create_insurance_contract(&env);
    let initial_capital = 2_000_0000000i128;
    contract.initialize(&admin, &token_addr, &initial_capital);
    let premium_amount = 50_0000000i128;
    let coverage_amount = 1_500_0000000i128;
    token_admin_client.mint(&customer, &premium_amount);
    let flight_id = String::from_str(&env, "G32102");
    let flight_date = env.ledger().timestamp() + (24 * 60 * 60);
    contract.create_policy(&customer, &flight_id, &flight_date, &premium_amount, &coverage_amount);
    let withdrawal = 1_000_0000000i128;
    contract.withdraw_from_pool(&withdrawal);
}