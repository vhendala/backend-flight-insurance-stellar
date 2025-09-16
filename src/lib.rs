#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, token, Address, Env, String, Vec,
};

// Enum para representar o status final de uma apólice
// CORREÇÃO: A variante 'Delayed' não deve carregar dados.
#[contracttype]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PolicyStatus {
    Unresolved,
    OnTime,
    Delayed,
    Cancelled,
}

// Estrutura representando uma apólice de seguro
#[contracttype]
#[derive(Clone)]
pub struct Policy {
    pub id: u64,
    pub customer: Address,
    pub flight_id: String,
    pub flight_date: u64,
    pub premium_amount: i128,
    pub coverage_amount: i128,
    pub status: PolicyStatus,
    pub payout_amount: i128,
}

// Enum para definir o tipo de resolução do voo
// CORREÇÃO: A variante 'Delayed' deve usar um campo de tupla.
#[contracttype]
pub enum FlightResolution {
    OnTime,
    Cancelled,
    Delayed(u64), 
}


// Chaves de armazenamento de dados do contrato
#[contracttype]
pub enum DataKey {
    Admin,
    UsdcToken,
    LiquidityPool,
    PolicyCounter,
    Policy(u64),
    ActivePolicies,
    FlightToPolicies(String),
}

#[contract]
pub struct FlightInsuranceContract;

#[contractimpl]
impl FlightInsuranceContract {
    /// Inicializa o contrato
    pub fn initialize(
        env: Env,
        admin: Address,
        usdc_token: Address,
        initial_capital: i128
    ) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("Contract already initialized");
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::UsdcToken, &usdc_token);
        env.storage().instance().set(&DataKey::LiquidityPool, &initial_capital);
        env.storage().instance().set(&DataKey::PolicyCounter, &0u64);
        env.storage().instance().set(&DataKey::ActivePolicies, &Vec::<u64>::new(&env));
    }

    /// Cria uma nova apólice de seguro
    pub fn create_policy(
        env: Env,
        customer: Address,
        flight_id: String,
        flight_date: u64,
        premium_amount: i128,
        coverage_amount: i128,
    ) -> u64 {
        customer.require_auth();

        if premium_amount <= 0 || coverage_amount <= 0 {
            panic!("Amounts must be positive");
        }
        if flight_date <= env.ledger().timestamp() {
            panic!("Flight date must be in the future");
        }

        let current_pool: i128 = env.storage().instance().get(&DataKey::LiquidityPool).unwrap_or(0);
        if current_pool < coverage_amount {
            panic!("Insufficient liquidity pool");
        }

        let usdc_token: Address = env.storage().instance().get(&DataKey::UsdcToken).expect("USDC token not configured");
        let token_client = token::Client::new(&env, &usdc_token);

        token_client.transfer(&customer, &env.current_contract_address(), &premium_amount);

        let new_pool = current_pool + premium_amount;
        env.storage().instance().set(&DataKey::LiquidityPool, &new_pool);

        let mut counter: u64 = env.storage().instance().get(&DataKey::PolicyCounter).unwrap_or(0);
        counter += 1;

        let new_policy = Policy {
            id: counter,
            customer: customer.clone(),
            flight_id: flight_id.clone(),
            flight_date,
            premium_amount,
            coverage_amount,
            status: PolicyStatus::Unresolved,
            payout_amount: 0,
        };

        env.storage().instance().set(&DataKey::Policy(counter), &new_policy);
        env.storage().instance().set(&DataKey::PolicyCounter, &counter);

        let mut active_policies: Vec<u64> = env.storage().instance().get(&DataKey::ActivePolicies).unwrap_or(Vec::new(&env));
        active_policies.push_back(counter);
        env.storage().instance().set(&DataKey::ActivePolicies, &active_policies);

        let flight_key = DataKey::FlightToPolicies(flight_id);
        let mut flight_policies: Vec<u64> = env.storage().instance().get(&flight_key).unwrap_or(Vec::new(&env));
        flight_policies.push_back(counter);
        env.storage().instance().set(&flight_key, &flight_policies);

        counter
    }

    /// Resolve todas as apólices de um voo específico
    pub fn resolve_flight(env: Env, flight_id: String, resolution: FlightResolution) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).expect("Admin not configured");
        admin.require_auth();

        let flight_key = DataKey::FlightToPolicies(flight_id.clone());
        let policy_ids: Vec<u64> = env.storage().instance().get(&flight_key).expect("No policies found for this flight");

        let usdc_token: Address = env.storage().instance().get(&DataKey::UsdcToken).expect("USDC token not configured");
        let token_client = token::Client::new(&env, &usdc_token);
        
        let mut current_pool: i128 = env.storage().instance().get(&DataKey::LiquidityPool).unwrap_or(0);

        for policy_id in policy_ids.iter() {
            let mut policy: Policy = env.storage().instance().get(&DataKey::Policy(policy_id)).expect("Policy not found");

            if policy.status != PolicyStatus::Unresolved {
                continue;
            }

            let mut payout = 0i128;
            
            match resolution {
                FlightResolution::Cancelled => {
                    policy.status = PolicyStatus::Cancelled;
                    payout = policy.premium_amount;
                },
                FlightResolution::OnTime => {
                    policy.status = PolicyStatus::OnTime;
                },
                FlightResolution::Delayed(delay_in_minutes) => {
                    // CORREÇÃO: Atribui o status simples e usa a variável do match
                    policy.status = PolicyStatus::Delayed;
                    if delay_in_minutes >= 60 && delay_in_minutes <= 180 { 
                        payout = policy.coverage_amount / 2; 
                    } else if delay_in_minutes > 180 { 
                        payout = policy.coverage_amount; 
                    }
                }
            }

            if payout > 0 {
                if current_pool < payout {
                    panic!("Insufficient pool for payout");
                }
                
                token_client.transfer(&env.current_contract_address(), &policy.customer, &payout);
                current_pool -= payout;
                policy.payout_amount = payout;
            }
            
            env.storage().instance().set(&DataKey::Policy(policy_id), &policy);

            let mut active_policies: Vec<u64> = env.storage().instance().get(&DataKey::ActivePolicies).unwrap_or(Vec::new(&env));
            if let Some(pos) = active_policies.iter().position(|x| x == policy_id) {
                active_policies.remove(pos as u32);
                env.storage().instance().set(&DataKey::ActivePolicies, &active_policies);
            }
        }
        
        env.storage().instance().set(&DataKey::LiquidityPool, &current_pool);
        
        env.storage().instance().remove(&flight_key);
    }
    
    /// Deposita fundos no pool
    pub fn deposit_to_pool(env: Env, amount: i128) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).expect("Admin not configured");
        admin.require_auth();

        if amount <= 0 {
            panic!("Amount must be positive");
        }

        let usdc_token: Address = env.storage().instance().get(&DataKey::UsdcToken).expect("USDC token not configured");
        let token_client = token::Client::new(&env, &usdc_token);
        token_client.transfer(&admin, &env.current_contract_address(), &amount);

        let current_pool: i128 = env.storage().instance().get(&DataKey::LiquidityPool).unwrap_or(0);
        let new_pool = current_pool + amount;
        env.storage().instance().set(&DataKey::LiquidityPool, &new_pool);
    }

    /// Retira fundos do pool
    pub fn withdraw_from_pool(env: Env, amount: i128) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).expect("Admin not configured");
        admin.require_auth();

        if amount <= 0 {
            panic!("Amount must be positive");
        }

        let current_pool: i128 = env.storage().instance().get(&DataKey::LiquidityPool).unwrap_or(0);
        let after_withdrawal = current_pool - amount;

        let active_policies: Vec<u64> = env.storage().instance().get(&DataKey::ActivePolicies).unwrap_or(Vec::new(&env));
        let mut total_exposure = 0i128;
        for id in active_policies.iter() {
            if let Some(policy) = env.storage().instance().get::<DataKey, Policy>(&DataKey::Policy(id)) {
                total_exposure += policy.coverage_amount;
            }
        }

        if after_withdrawal < total_exposure {
            panic!("Withdrawal would compromise active policies coverage");
        }

        let usdc_token: Address = env.storage().instance().get(&DataKey::UsdcToken).expect("USDC token not configured");
        let token_client = token::Client::new(&env, &usdc_token);
        token_client.transfer(&env.current_contract_address(), &admin, &amount);

        env.storage().instance().set(&DataKey::LiquidityPool, &after_withdrawal);
    }

    // === FUNÇÕES DE CONSULTA ===

    /// Obtém detalhes da apólice
    pub fn get_policy(env: Env, policy_id: u64) -> Policy {
        env.storage().instance().get(&DataKey::Policy(policy_id)).expect("Policy not found")
    }

    /// Obtém o saldo atual do pool de liquidez
    pub fn get_liquidity_pool(env: Env) -> i128 {
        env.storage().instance().get(&DataKey::LiquidityPool).unwrap_or(0)
    }

    /// Obtém a lista de IDs de apólices ativas
    pub fn get_active_policies(env: Env) -> Vec<u64> {
        env.storage().instance().get(&DataKey::ActivePolicies).unwrap_or(Vec::new(&env))
    }
    
    /// Obtém a lista de IDs de apólices para um voo específico
    pub fn get_policies_for_flight(env: Env, flight_id: String) -> Vec<u64> {
        env.storage().instance().get(&DataKey::FlightToPolicies(flight_id)).unwrap_or(Vec::new(&env))
    }

    /// Obtém o total de apólices criadas
    pub fn get_total_policies(env: Env) -> u64 {
        env.storage().instance().get(&DataKey::PolicyCounter).unwrap_or(0)
    }

    /// Verifica se o endereço é de um administrador
    pub fn is_admin(env: Env, address: Address) -> bool {
        if let Some(admin) = env.storage().instance().get::<DataKey, Address>(&DataKey::Admin) {
            admin == address
        } else {
            false
        }
    }
}