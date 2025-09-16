#![cfg(test)]

use super::*;
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{symbol_short, token, Address, Env};

// Helper para criar e configurar o contrato em um ambiente de teste
fn setup_contract<'a>() -> (
    Env,
    FlightInsuranceContractClient<'a>,
    Address,
    Address,
    token::Client<'a>,
) {
    let env = Env::default();
    env.ledger().set(LedgerInfo {
        timestamp: 1726500000, // 16 de Setembro de 2025
        protocol_version: 20,
        sequence_number: 10,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 1,
        min_persistent_entry_ttl: 1,
    });

    let contract_id = env.register_contract(None, FlightInsuranceContract);
    let client = FlightInsuranceContractClient::new(&env, &contract_id);

    let admin = Address::random(&env);
    let usdc_token_id = env.register_stellar_asset_contract(admin.clone());
    let usdc_token = token::Client::new(&env, &usdc_token_id);

    // O pool inicial é transferido externamente, então apenas inicializamos o valor
    let initial_capital = 10_000 * 1_0000000; // 10,000 USDC
    usdc_token.mint(&contract_id, &initial_capital);

    client.initialize(&admin, &usdc_token_id, &initial_capital);

    (env, client, admin, usdc_token_id, usdc_token)
}

#[test]
fn test_initialize() {
    let (env, client, admin, usdc_token_id, _) = setup_contract();

    // Verifica se os valores foram salvos corretamente
    assert_eq!(client.is_admin(&admin), true);
    assert_eq!(
        client.get_liquidity_pool(),
        10_000 * 1_0000000
    );

    // Verifica se chamar initialize de novo causa pânico
    let result = env.try_invoke_contract_fn(
        &client.address,
        symbol_short!("initialize"),
        (admin, usdc_token_id, 1000i128).into_val(&env),
    );
    assert!(result.is_err());
}

#[test]
fn test_create_policy() {
    let (env, client, _, usdc_token_id, usdc_token) = setup_contract();

    let customer = Address::random(&env);
    let premium = 50 * 1_0000000; // 50 USDC
    let coverage = 500 * 1_0000000; // 500 USDC
    let flight_date = env.ledger().timestamp() + (48 * 60 * 60); // 48 horas no futuro

    // Dando fundos ao cliente
    usdc_token.mint(&customer, &(premium + 10_0000000));

    // Cliente (customer) precisa autorizar a chamada
    env.as_contract(&client.address, || {
        let policy_id = client
            .with_source_account(&customer)
            .create_policy(
                &customer,
                &"FL123".into_val(&env),
                &flight_date,
                &premium,
                &coverage,
            );

        assert_eq!(policy_id, 1);

        // Verifica a apólice
        let policy = client.get_policy(&policy_id);
        assert_eq!(policy.customer, customer);
        assert_eq!(policy.premium_amount, premium);
        assert_eq!(policy.status, PolicyStatus::Unresolved);

        // Verifica se o prêmio foi transferido
        assert_eq!(usdc_token.balance(&customer), 10_0000000);
        assert_eq!(
            usdc_token.balance(&client.address),
            10_000 * 1_0000000 + premium
        );

        // Verifica se o pool de liquidez foi atualizado
        assert_eq!(
            client.get_liquidity_pool(),
            10_000 * 1_0000000 + premium
        );
        
        // Verifica se a apólice está na lista de ativas e no mapeamento de voos
        assert_eq!(client.get_active_policies().len(), 1);
        assert_eq!(client.get_policies_for_flight(&"FL123".into_val(&env)).len(), 1);
    });
}


#[test]
#[should_panic(expected = "Insufficient liquidity pool")]
fn test_create_policy_insufficient_liquidity() {
    let (env, client, _, _, _) = setup_contract();
    let customer = Address::random(&env);
    
    // Cobertura maior que o pool inicial
    let coverage = 20_000 * 1_0000000; 

    client.with_source_account(&customer).create_policy(
        &customer,
        &"FL999".into_val(&env),
        &(env.ledger().timestamp() + 1000),
        &(100 * 1_0000000),
        &coverage,
    );
}

#[test]
fn test_resolve_flight_on_time() {
    let (env, client, admin, _, usdc_token) = setup_contract();
    let customer = Address::random(&env);
    let premium = 50 * 1_0000000;
    usdc_token.mint(&customer, &premium);

    let policy_id = client.with_source_account(&customer).create_policy(
        &customer, &"FL456".into_val(&env), &(env.ledger().timestamp() + 1000), &premium, &(500 * 1_0000000)
    );

    let initial_pool = client.get_liquidity_pool();
    
    client.with_source_account(&admin).resolve_flight(&"FL456".into_val(&env), &FlightResolution::OnTime);

    let policy = client.get_policy(&policy_id);
    assert_eq!(policy.status, PolicyStatus::OnTime);
    assert_eq!(policy.payout_amount, 0);

    // Pool não mudou (lucrou o prêmio)
    assert_eq!(client.get_liquidity_pool(), initial_pool);
    // Cliente não recebeu nada de volta
    assert_eq!(usdc_token.balance(&customer), 0);
    // Apólice não está mais ativa
    assert_eq!(client.get_active_policies().len(), 0);
    assert_eq!(client.get_policies_for_flight(&"FL456".into_val(&env)).len(), 0);
}

#[test]
fn test_resolve_flight_cancelled() {
    let (env, client, admin, _, usdc_token) = setup_contract();
    let customer = Address::random(&env);
    let premium = 50 * 1_0000000;
    usdc_token.mint(&customer, &premium);

    let policy_id = client.with_source_account(&customer).create_policy(
        &customer, &"FL789".into_val(&env), &(env.ledger().timestamp() + 1000), &premium, &(500 * 1_0000000)
    );

    let initial_pool_before_premium = 10_000 * 1_0000000;
    
    client.with_source_account(&admin).resolve_flight(&"FL789".into_val(&env), &FlightResolution::Cancelled);

    let policy = client.get_policy(&policy_id);
    assert_eq!(policy.status, PolicyStatus::Cancelled);
    assert_eq!(policy.payout_amount, premium);

    // Pool voltou ao estado original (prêmio foi devolvido)
    assert_eq!(client.get_liquidity_pool(), initial_pool_before_premium);
    // Cliente recebeu o prêmio de volta
    assert_eq!(usdc_token.balance(&customer), premium);
    assert_eq!(client.get_active_policies().len(), 0);
}

#[test]
fn test_resolve_flight_delayed_partial_payout() {
    let (env, client, admin, _, usdc_token) = setup_contract();
    let customer = Address::random(&env);
    let premium = 50 * 1_0000000;
    let coverage = 500 * 1_0000000;
    let expected_payout = coverage / 2; // 50%
    usdc_token.mint(&customer, &premium);

    let policy_id = client.with_source_account(&customer).create_policy(
        &customer, &"FL-D1".into_val(&env), &(env.ledger().timestamp() + 1000), &premium, &coverage
    );

    let pool_after_premium = client.get_liquidity_pool();
    
    // Atraso de 90 minutos
    client.with_source_account(&admin).resolve_flight(&"FL-D1".into_val(&env), &FlightResolution::Delayed(90));

    let policy = client.get_policy(&policy_id);
    assert_eq!(policy.status, PolicyStatus::Delayed);
    assert_eq!(policy.payout_amount, expected_payout);

    // Pool foi reduzido pelo pagamento
    assert_eq!(client.get_liquidity_pool(), pool_after_premium - expected_payout);
    // Cliente recebeu o pagamento
    assert_eq!(usdc_token.balance(&customer), expected_payout);
}

#[test]
fn test_resolve_flight_delayed_full_payout() {
    let (env, client, admin, _, usdc_token) = setup_contract();
    let customer = Address::random(&env);
    let premium = 50 * 1_0000000;
    let coverage = 500 * 1_0000000;
    usdc_token.mint(&customer, &premium);

    let policy_id = client.with_source_account(&customer).create_policy(
        &customer, &"FL-D2".into_val(&env), &(env.ledger().timestamp() + 1000), &premium, &coverage
    );

    let pool_after_premium = client.get_liquidity_pool();
    
    // Atraso de 200 minutos
    client.with_source_account(&admin).resolve_flight(&"FL-D2".into_val(&env), &FlightResolution::Delayed(200));

    let policy = client.get_policy(&policy_id);
    assert_eq!(policy.status, PolicyStatus::Delayed);
    assert_eq!(policy.payout_amount, coverage);

    assert_eq!(client.get_liquidity_pool(), pool_after_premium - coverage);
    assert_eq!(usdc_token.balance(&customer), coverage);
}

#[test]
fn test_resolve_multiple_policies_for_same_flight() {
    let (env, client, admin, _, usdc_token) = setup_contract();
    
    let customer1 = Address::random(&env);
    let customer2 = Address::random(&env);
    let premium = 20 * 1_0000000;
    let coverage = 200 * 1_0000000;
    let flight_id = "FL-MULTI".into_val(&env);

    usdc_token.mint(&customer1, &premium);
    usdc_token.mint(&customer2, &premium);

    let policy1_id = client.with_source_account(&customer1).create_policy(
        &customer1, &flight_id, &(env.ledger().timestamp() + 1000), &premium, &coverage
    );
    let policy2_id = client.with_source_account(&customer2).create_policy(
        &customer2, &flight_id, &(env.ledger().timestamp() + 1000), &premium, &coverage
    );

    assert_eq!(client.get_active_policies().len(), 2);
    assert_eq!(client.get_policies_for_flight(&flight_id).len(), 2);

    let pool_after_premiums = client.get_liquidity_pool();
    
    // Voo cancelado, ambos devem ser reembolsados
    client.with_source_account(&admin).resolve_flight(&flight_id, &FlightResolution::Cancelled);

    // Verifica apólice 1
    let p1 = client.get_policy(&policy1_id);
    assert_eq!(p1.status, PolicyStatus::Cancelled);
    assert_eq!(p1.payout_amount, premium);

    // Verifica apólice 2
    let p2 = client.get_policy(&policy2_id);
    assert_eq!(p2.status, PolicyStatus::Cancelled);
    assert_eq!(p2.payout_amount, premium);

    // Verifica balanços
    assert_eq!(usdc_token.balance(&customer1), premium);
    assert_eq!(usdc_token.balance(&customer2), premium);

    // Verifica pool final
    assert_eq!(client.get_liquidity_pool(), pool_after_premiums - (2 * premium));

    // Verifica limpeza das listas
    assert_eq!(client.get_active_policies().len(), 0);
    assert_eq!(client.get_policies_for_flight(&flight_id).len(), 0);
}


#[test]
#[should_panic]
fn test_resolve_flight_not_admin() {
    let (env, client, _, _, _) = setup_contract();
    let not_admin = Address::random(&env);
    // Tenta resolver sem ser admin
    client.with_source_account(&not_admin).resolve_flight(&"FL123".into_val(&env), &FlightResolution::OnTime);
}