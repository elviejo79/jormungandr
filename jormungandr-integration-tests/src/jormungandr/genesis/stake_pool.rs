use crate::common::{
    configuration::genesis_model::{Fund, LinearFees},
    data::address::Account,
    file_utils,
    jcli_wrapper::{
        self, certificate::wrapper::JCLICertificateWrapper,
        jcli_transaction_wrapper::JCLITransactionWrapper,
    },
    jormungandr::{ConfigurationBuilder, Starter},
    process_utils::Wait,
    startup,
};

use chain_addr::Discrimination;
use chain_crypto::{Curve25519_2HashDH, SumEd25519_12};
use jormungandr_lib::{crypto::hash::Hash, interfaces::Value};
use std::str::FromStr;

#[allow(dead_code)]
fn create_account_from_secret_key(private_key: String) -> Account {
    let public_key = jcli_wrapper::assert_key_to_public_default(&private_key);
    let address = jcli_wrapper::assert_address_single(&public_key, Discrimination::Test);
    Account::new(&private_key, &public_key, &address)
}

#[test]
pub fn create_delegate_retire_stake_pool() {
    let mut actor_account = startup::create_new_account_address();

    let config = ConfigurationBuilder::new()
        .with_linear_fees(LinearFees {
            constant: 100,
            coefficient: 100,
            certificate: 200,
        })
        .with_funds(vec![Fund {
            value: 1000000.into(),
            address: actor_account.address.clone(),
        }])
        .build();

    let jormungandr = Starter::new().config(config.clone()).start().unwrap();
    let block0_hash = config.genesis_block_hash;

    let stake_pool_id = create_new_stake_pool(
        &mut actor_account,
        &block0_hash,
        &jormungandr.rest_address(),
        &Default::default(),
    );
    delegate_stake(
        &mut actor_account,
        &stake_pool_id,
        &block0_hash,
        &jormungandr.rest_address(),
        &Default::default(),
    );
    retire_stake_pool(
        &stake_pool_id,
        &mut actor_account,
        &block0_hash,
        &jormungandr.rest_address(),
        &Default::default(),
    );
}

pub fn create_new_stake_pool(
    account: &mut Account,
    genesis_block_hash: &str,
    jormungandr_rest_address: &str,
    wait: &Wait,
) -> String {
    let kes = startup::create_new_key_pair::<Curve25519_2HashDH>();
    let vrf = startup::create_new_key_pair::<SumEd25519_12>();

    let owner_stake_key =
        file_utils::create_file_in_temp("stake_key.private_key", &account.private_key);

    let settings = jcli_wrapper::assert_get_rest_settings(&jormungandr_rest_address);
    let fees: LinearFees = settings.fees.into();
    let fee_value: Value = (fees.certificate + fees.coefficient + fees.constant).into();

    let certificate_wrapper = JCLICertificateWrapper::new();

    let stake_pool_certificate = certificate_wrapper.assert_new_stake_pool_registration(
        &vrf.identifier().to_bech32_str(),
        &kes.identifier().to_bech32_str(),
        0u32,
        1u32,
        &account.public_key,
    );
    let stake_pool_certificate_file =
        file_utils::create_file_in_temp("stake_pool.cert", &stake_pool_certificate);

    let transaction = JCLITransactionWrapper::new_transaction(genesis_block_hash)
        .assert_add_account(&account.address, &fee_value)
        .assert_add_certificate(&stake_pool_certificate)
        .assert_finalize_with_fee(&account.address, &fees)
        .seal_with_witness_for_address(account)
        .assert_add_auth(&owner_stake_key)
        .assert_to_message();

    account.confirm_transaction();
    jcli_wrapper::assert_transaction_in_block_with_wait(
        &transaction,
        &jormungandr_rest_address,
        wait,
    );

    let stake_pool_id = certificate_wrapper.assert_get_stake_pool_id(&stake_pool_certificate_file);

    assert!(
        jcli_wrapper::assert_rest_get_stake_pools(&jormungandr_rest_address)
            .contains(&stake_pool_id),
        "cannot find stake-pool certificate in blockchain"
    );

    stake_pool_id.to_owned()
}

pub fn delegate_stake(
    account: &mut Account,
    stake_pool_id: &str,
    genesis_block_hash: &str,
    jormungandr_rest_address: &str,
    wait: &Wait,
) {
    let owner_stake_key =
        file_utils::create_file_in_temp("stake_key.private_key", &account.private_key);
    let certificate_wrapper = JCLICertificateWrapper::new();
    let stake_pool_delegation =
        certificate_wrapper.assert_new_stake_delegation(&stake_pool_id, &account.public_key);

    let settings = jcli_wrapper::assert_get_rest_settings(&jormungandr_rest_address);
    let fees: LinearFees = settings.fees.into();
    let fee_value: Value = (fees.certificate + fees.coefficient + fees.constant).into();

    let transaction = JCLITransactionWrapper::new_transaction(genesis_block_hash)
        .assert_add_account(&account.address, &fee_value)
        .assert_add_certificate(&stake_pool_delegation)
        .assert_finalize_with_fee(&account.address, &fees)
        .seal_with_witness_for_address(account)
        .assert_add_auth(&owner_stake_key)
        .assert_to_message();

    account.confirm_transaction();
    jcli_wrapper::assert_transaction_in_block_with_wait(
        &transaction,
        &jormungandr_rest_address,
        wait,
    );

    let account_state_after_delegation =
        jcli_wrapper::assert_rest_account_get_stats(&account.address, &jormungandr_rest_address);

    let stake_pool_id_hash = Hash::from_str(&stake_pool_id).unwrap();
    assert!(
        account_state_after_delegation
            .delegation()
            .pools()
            .iter()
            .any(|(hash, _)| *hash == stake_pool_id_hash),
        "account should be delegated to pool"
    );
}

pub fn retire_stake_pool(
    stake_pool_id: &str,
    account: &mut Account,
    genesis_block_hash: &str,
    jormungandr_rest_address: &str,
    wait: &Wait,
) {
    let owner_private_key =
        file_utils::create_file_in_temp("stake_key.private_key", &account.private_key);

    let certificate_wrapper = JCLICertificateWrapper::new();

    let retirement_cert = certificate_wrapper.assert_new_stake_pool_retirement(&stake_pool_id);

    let settings = jcli_wrapper::assert_get_rest_settings(&jormungandr_rest_address);
    let fees: LinearFees = settings.fees.into();
    let fee_value: Value = (fees.certificate + fees.coefficient + fees.constant).into();

    let transaction = JCLITransactionWrapper::new_transaction(genesis_block_hash)
        .assert_add_account(&account.address, &fee_value)
        .assert_add_certificate(&retirement_cert)
        .assert_finalize_with_fee(&account.address, &fees)
        .seal_with_witness_for_address(account)
        .assert_add_auth(&owner_private_key)
        .assert_to_message();

    account.confirm_transaction();
    jcli_wrapper::assert_transaction_in_block_with_wait(
        &transaction,
        &jormungandr_rest_address,
        wait,
    );

    let account_state_after_stake_pool_retire =
        jcli_wrapper::assert_rest_account_get_stats(&account.address, &jormungandr_rest_address);

    let stake_pool_id_hash = Hash::from_str(&stake_pool_id).unwrap();

    assert!(
        account_state_after_stake_pool_retire
            .delegation()
            .pools()
            .iter()
            .any(|(hash, _)| *hash == stake_pool_id_hash),
        "account should be still delegated to retired pool"
    );

    assert!(
        !jcli_wrapper::assert_rest_get_stake_pools(&jormungandr_rest_address)
            .contains(&stake_pool_id.to_owned()),
        "stake pool should not be listed among active stake pools"
    );
}
