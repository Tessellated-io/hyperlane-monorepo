use async_trait::async_trait;
use ed25519_dalek::SecretKey;
use ethers::prelude::{AwsSigner, LocalWallet, YubiWallet};
use ethers::signers::yubihsm::authentication::Key as AuthenticationKey;
use ethers::signers::yubihsm::ecdsa::Signer;
use ethers::signers::yubihsm::{
    ecdsa::Signer as YubiSigner, Client, Connector, Credentials, HttpConfig,
};
use ethers::signers::Wallet;
use ethers::utils::hex::ToHex;
use eyre::{bail, Context, Report};
use hyperlane_core::{AccountAddressType, H256};
use hyperlane_sealevel::Keypair;
use lazy_static::lazy_static;
use rusoto_core::Region;
use rusoto_kms::KmsClient;
use std::sync::Mutex;
use std::sync::OnceLock;
use tracing::instrument;
use tracing::{error, info, info_span, instrument::Instrumented, warn, Instrument};
// use yubihsm::{
//     asymmetric::Algorithm::EcK256, object, object::Label, Capability, Client, Connector,
//     Credentials, Domain,
// };
use std::thread;
use std::time::Duration;

use super::aws_credentials::AwsChainCredentialsProvider;
use crate::types::utils;

// Global var for yubishm connector.
// Creating connectors for each validator causes the number of sessions the device can support to become exhausted.
// static mut YUBIHSM_SIGNER: Box<YubiWallet> = Box::<YubiWallet>::new_uninit();
// lazy_static! {
//     static ref YUBIHSM_SIGNER: Option<Box<YubiWallet>> = None;
// }

/// Signer types
#[derive(Default, Debug, Clone)]
pub enum SignerConf {
    /// A local hex key
    HexKey {
        /// Private key value
        key: H256,
    },
    /// An AWS signer. Note that AWS credentials must be inserted into the env
    /// separately.
    Aws {
        /// The UUID identifying the AWS KMS Key
        id: String,
        /// The AWS region
        region: Region,
    },
    /// A Yubihsm2 signer
    YubiHsm {
        /// Port to access the YubiHSM HTTP Connector
        port: u16,
        /// Authentication key id
        authentication_key_id: u16,
        /// Authentication key password
        password: String,
        /// Signer key id
        signer_key_id: u16,
    },
    /// Cosmos Specific key
    CosmosKey {
        /// Private key value
        key: H256,
        /// Prefix for cosmos address
        prefix: String,
        /// Account address type for cosmos address
        account_address_type: AccountAddressType,
    },
    /// Assume node will sign on RPC calls
    #[default]
    Node,
}

impl SignerConf {
    /// Try to convert the ethereum signer to a local wallet
    #[instrument(err)]
    pub async fn build<S: BuildableWithSignerConf>(&self) -> Result<S, Report> {
        S::build(self).await
    }
}

/// A signer for a chain.
pub trait ChainSigner: Send {
    /// The address of the signer, formatted in the chain's own address format.
    fn address_string(&self) -> String;
}

/// Builder trait for signers
#[async_trait]
pub trait BuildableWithSignerConf: Sized + ChainSigner {
    /// Build a signer from a conf
    async fn build(conf: &SignerConf) -> Result<Self, Report>;
}

// use std::sync::Once;

// static INIT: Once = Once::new();
// static mut YUBIHSM_SIGNER: Option<hyperlane_ethereum::Signers> = None;

// fn get_or_init_yubihsm_signer(
//     port: &u16,
//     authentication_key_id: &u16,
//     password: &String,
//     signer_key_id: &u16,
// ) -> &'static hyperlane_ethereum::Signers {
//     unsafe {
//         INIT.call_once(|| {
//

//
//             let credentials = ethers::signers::yubihsm::Credentials::new(
//                 *authentication_key_id,
//                 authentication_key,
//             );

//             // Initialize the YubiWallet
//             let signer_instance = YubiWallet::connect(connector, credentials, *signer_key_id);

//             // Store the signer wrapped in the enum
//             YUBIHSM_SIGNER = Some(hyperlane_ethereum::Signers::YubiHsm(signer_instance));
//         });

//         // Return a reference to the initialized signer
//         YUBIHSM_SIGNER.as_ref().unwrap()
//     }
// }

static mut X: Option<Client> = None;

#[async_trait]
impl BuildableWithSignerConf for hyperlane_ethereum::Signers {
    async fn build(conf: &SignerConf) -> Result<Self, Report> {
        Ok(match conf {
            SignerConf::HexKey { key } => hyperlane_ethereum::Signers::Local(LocalWallet::from(
                ethers::core::k256::ecdsa::SigningKey::from(
                    ethers::core::k256::SecretKey::from_be_bytes(key.as_bytes())
                        .context("Invalid ethereum signer key")?,
                ),
            )),
            SignerConf::Aws { id, region } => {
                let client = KmsClient::new_with_client(
                    rusoto_core::Client::new_with(
                        AwsChainCredentialsProvider::new("TODO", "TODO"),
                        utils::http_client_with_timeout().unwrap(),
                    ),
                    region.clone(),
                );

                let signer = AwsSigner::new(client, id, 0).await?;
                hyperlane_ethereum::Signers::Aws(signer)
            }
            SignerConf::YubiHsm {
                port,
                authentication_key_id,
                password,
                signer_key_id,
            } => {
                unsafe {
                    if X.is_none() {
                        let http_config = ethers::signers::yubihsm::HttpConfig {
                            addr: "127.0.0.1".to_owned(),
                            port: *port,
                            timeout_ms: 5000,
                        };
                        let connector = ethers::signers::yubihsm::Connector::http(&http_config);

                        let authentication_key =
                            ethers::signers::yubihsm::authentication::Key::derive_from_password(
                                password.as_bytes(),
                            );
                        let credentials = ethers::signers::yubihsm::Credentials::new(
                            *authentication_key_id,
                            authentication_key,
                        );

                        // let wallet = YubiWallet::connect(connector, credentials, *signer_key_id);

                        let client_result = Client::create(connector, credentials);
                        let client = client_result.unwrap();

                        X = Some(client);
                        info!(port, "connected to yubihsm2");
                    }

                    let x = X.clone();
                    let yubi_signer = YubiSigner::create(x.unwrap(), *signer_key_id).unwrap();
                    let wallet2 = Wallet::from(yubi_signer);

                    // Sleep for 5 seconds to let the connection drop
                    // TODO: doc why
                    // thread::sleep(Duration::new(5, 0)); // Sleep for 2 seconds

                    // // this will never fail
                    // let public_key = PublicKey::from_encoded_point(signer.public_key()).unwrap();
                    // let public_key = public_key.to_encoded_point(/* compress = */ false);
                    // let public_key = public_key.as_bytes();
                    // debug_assert_eq!(public_key[0], 0x04);
                    // let hash = keccak256(&public_key[1..]);
                    // let address = Address::from_slice(&hash[12..]);

                    // // Self { signer, address, chain_id: 1 }

                    // let wallet = Wallet::new_with_signer(yubi_signer, address, 1);
                    hyperlane_ethereum::Signers::YubiHsm(Box::new(wallet2)) //todo: no box
                }
            }
            SignerConf::CosmosKey { .. } => {
                bail!("cosmosKey signer is not supported by Ethereum")
            }
            SignerConf::Node => bail!("Node signer"),
        })
    }
}

impl ChainSigner for hyperlane_ethereum::Signers {
    fn address_string(&self) -> String {
        ethers::signers::Signer::address(self).encode_hex()
    }
}

#[async_trait]
impl BuildableWithSignerConf for fuels::prelude::WalletUnlocked {
    async fn build(conf: &SignerConf) -> Result<Self, Report> {
        if let SignerConf::HexKey { key } = conf {
            let key = fuels::crypto::SecretKey::try_from(key.as_bytes())
                .context("Invalid fuel signer key")?;
            Ok(fuels::prelude::WalletUnlocked::new_from_private_key(
                key, None,
            ))
        } else {
            bail!(format!("{conf:?} key is not supported by fuel"));
        }
    }
}

impl ChainSigner for fuels::prelude::WalletUnlocked {
    fn address_string(&self) -> String {
        self.address().to_string()
    }
}

#[async_trait]
impl BuildableWithSignerConf for Keypair {
    async fn build(conf: &SignerConf) -> Result<Self, Report> {
        if let SignerConf::HexKey { key } = conf {
            let secret = SecretKey::from_bytes(key.as_bytes())
                .context("Invalid sealevel ed25519 secret key")?;
            let public = ed25519_dalek::PublicKey::from(&secret);
            let dalek = ed25519_dalek::Keypair { secret, public };
            Ok(Keypair::from_bytes(&dalek.to_bytes()).context("Unable to create Keypair")?)
        } else {
            bail!(format!("{conf:?} key is not supported by sealevel"));
        }
    }
}

impl ChainSigner for Keypair {
    fn address_string(&self) -> String {
        solana_sdk::signer::Signer::pubkey(self).to_string()
    }
}

#[async_trait]
impl BuildableWithSignerConf for hyperlane_cosmos::Signer {
    async fn build(conf: &SignerConf) -> Result<Self, Report> {
        if let SignerConf::CosmosKey {
            key,
            prefix,
            account_address_type,
        } = conf
        {
            Ok(hyperlane_cosmos::Signer::new(
                key.as_bytes().to_vec(),
                prefix.clone(),
                account_address_type,
            )?)
        } else {
            bail!(format!("{conf:?} key is not supported by cosmos"));
        }
    }
}

impl ChainSigner for hyperlane_cosmos::Signer {
    fn address_string(&self) -> String {
        self.address.clone()
    }
}
