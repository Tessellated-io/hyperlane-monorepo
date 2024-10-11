//! Validator configuration.
//!
//! The correct settings shape is defined in the TypeScript SDK metadata. While the exact shape
//! and validations it defines are not applied here, we should mirror them.
//! ANY CHANGES HERE NEED TO BE REFLECTED IN THE TYPESCRIPT SDK.

use std::{collections::HashSet, path::PathBuf, time::Duration};

use itertools::Itertools;

use derive_more::{AsMut, AsRef, Deref, DerefMut};
use eyre::{eyre, Context};
use hyperlane_base::{
    impl_loadable_from_settings,
    settings::{
        parser::{RawAgentConf, RawAgentSignerConf, ValueParser},
        ChainConnectionConf, CheckpointSyncerConf, Settings, SignerConf,
    },
};
use hyperlane_core::{cfg_unwrap_all, config::*, HyperlaneDomain, HyperlaneDomainProtocol};
use hyperlane_cosmos::{NativeToken, RawCosmosAmount};
use hyperlane_ethereum::{ConnectionConf, RpcConnectionConf};
use serde::Deserialize;
use serde_json::Value;
use tracing::{error, info, info_span, instrument::Instrumented, warn, Instrument};

use hyperlane_core::IndexMode;
use url::Url;

use std::{collections::HashMap, default::Default};
/// Settings for `Validator`
#[derive(Debug, AsRef, AsMut, Deref, DerefMut)]
pub struct ValidatorSettings {
    #[as_ref]
    #[as_mut]
    #[deref]
    #[deref_mut]
    base: Settings,

    pub validators: Vec<WrappedValidatorSettings>,
}

#[derive(Debug)]
pub struct WrappedValidatorSettings {
    /// Database path
    pub db: PathBuf,
    /// Chain to validate messages on
    pub origin_chain: HyperlaneDomain,
    /// The validator attestation signer
    pub validator: SignerConf,
    /// The checkpoint syncer configuration
    pub checkpoint_syncer: CheckpointSyncerConf,
    /// The reorg_period in blocks
    pub reorg_period: u64,
    /// How frequently to check for new checkpoints
    pub interval: Duration,
}

impl Clone for WrappedValidatorSettings {
    fn clone(&self) -> Self {
        Self {
            db: self.db.clone(),
            origin_chain: self.origin_chain.clone(),
            validator: self.validator.clone(),
            checkpoint_syncer: self.checkpoint_syncer.clone(),
            reorg_period: self.reorg_period.clone(),
            interval: self.interval.clone(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(transparent)]
struct RawValidatorSettings(Value);

impl_loadable_from_settings!(Validator, RawValidatorSettings -> ValidatorSettings);

impl FromRawConf<RawValidatorSettings> for ValidatorSettings {
    fn from_config_filtered(
        raw: RawValidatorSettings,
        cwp: &ConfigPath,
        _filter: (),
    ) -> ConfigResult<Self> {
        println!("running raw config load {}", raw.0);

        let mut err = ConfigParsingError::default();

        let p = ValueParser::new(cwp.clone(), &raw.0);

        // let base = parse_base(p.clone(), cwp);

        // let origin_chain_name_set = origin_chain_name.map(|s| HashSet::from([s]));

        let base: Option<Settings> = p
            .parse_from_raw_config::<Settings, RawAgentConf, Option<&HashSet<&str>>>(
                None, // origin_chain_name_set.as_ref(),
                "Expected valid base agent configuration",
            )
            .take_config_err(&mut err);
        let unwrapped_base = base.unwrap();

        // let unwrappedBase = base.unwrap();
        // info!(unwrappedBase.chains["ethereum"].connection);

        let validator_parsers: Vec<ValueParser> = p
            .chain(&mut err)
            .get_key("validators")
            .into_array_iter()
            .unwrap()
            .collect();

        let mut validators: Vec<WrappedValidatorSettings> = vec![];
        for parser in validator_parsers {
            let validator: WrappedValidatorSettings =
                parse_validator(parser.clone(), Some(&unwrapped_base), cwp);
            validators.push(validator);
        }

        err.into_result(Self {
            base: unwrapped_base,
            validators,
        })
    }
}

fn parse_validator(
    p: ValueParser,
    base: Option<&Settings>,
    cwp: &ConfigPath,
) -> WrappedValidatorSettings {
    let mut err = ConfigParsingError::default();
    println!("running parse validator");

    let checkpoint_syncer = p
        .chain(&mut err)
        .get_key("checkpointSyncer")
        .and_then(parse_checkpoint_syncer)
        .end();

    let origin_chain_name = p
        .chain(&mut err)
        .get_key("originChainName")
        .parse_string()
        .end();
    println!("read origin chain as {}", origin_chain_name.unwrap());

    let db = p
        .chain(&mut err)
        .get_opt_key("db")
        .parse_from_str("Expected db file path")
        .unwrap_or_else(|| {
            std::env::current_dir()
                .unwrap()
                .join(format!("validator_db_{}", origin_chain_name.unwrap_or("")))
        });

    let interval = p
        .chain(&mut err)
        .get_opt_key("interval")
        .parse_u64()
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(5));

    let origin_chain = if let (Some(base), Some(origin_chain_name)) = (&base, origin_chain_name) {
        base.lookup_domain(origin_chain_name)
            .context("Missing configuration for the origin chain")
            .take_err(&mut err, || cwp + "origin_chain_name")
    } else {
        None
    };

    let validator = p
        .chain(&mut err)
        .get_key("validator")
        .parse_from_raw_config::<SignerConf, RawAgentSignerConf, NoFilter>(
            (),
            "Expected valid validator configuration",
        )
        .end();
    let reorg_period = p
        .chain(&mut err)
        .get_key("chains")
        .get_key(origin_chain_name.unwrap())
        .get_opt_key("blocks")
        .get_opt_key("reorgPeriod")
        .parse_u64()
        .unwrap_or(1);

    let validator_settings: WrappedValidatorSettings = WrappedValidatorSettings {
        db: db.clone(),
        checkpoint_syncer: checkpoint_syncer.unwrap().clone(),
        interval,
        origin_chain: origin_chain.unwrap().clone(),
        validator: validator.unwrap().clone(),
        reorg_period,
    };

    return validator_settings;
}

fn parse_base(p: ValueParser, cwp: &ConfigPath) -> hyperlane_base::settings::Settings {
    let mut err = ConfigParsingError::default();
    println!("parsing configs!");

    warn!("parsing configs");

    let metrics_port = p
        .chain(&mut err)
        .get_opt_key("metricsPort")
        .parse_u16()
        .unwrap_or(9090);

    let fmt = p
        .chain(&mut err)
        .get_opt_key("log")
        .get_opt_key("format")
        .parse_value("Invalid log format")
        .unwrap_or_default();

    let level = p
        .chain(&mut err)
        .get_opt_key("log")
        .get_opt_key("level")
        .parse_value("Invalid log level")
        .unwrap_or_default();

    let raw_chains: Vec<(String, ValueParser)> = p
        .chain(&mut err)
        .get_opt_key("chains")
        .into_obj_iter()
        .map(|v| v.collect())
        .unwrap_or_default();

    let default_signer = p
        .chain(&mut err)
        .get_opt_key("defaultSigner")
        .and_then(parse_signer)
        .end();

    let default_rpc_consensus_type = p
        .chain(&mut err)
        .get_opt_key("defaultRpcConsensusType")
        .parse_string()
        .unwrap_or("fallback");

    let chains: HashMap<String, hyperlane_base::settings::ChainConf> = raw_chains
        .into_iter()
        .filter_map(|(name, chain)| {
            parse_chain(chain, &name, default_rpc_consensus_type)
                .take_config_err(&mut err)
                .map(|v| (name, v))
        })
        .map(|(name, mut chain)| {
            if let Some(default_signer) = &default_signer {
                println!("found a signer attached to chain {name}");
                chain.signer.get_or_insert_with(|| default_signer.clone());
            }
            (name, chain)
        })
        .collect();

    return hyperlane_base::settings::Settings {
        chains,
        metrics_port,
        tracing: hyperlane_base::settings::TracingConfig { fmt, level },
    };
}

/// The chain name and ChainMetadata
fn parse_chain(
    chain: ValueParser,
    name: &str,
    default_rpc_consensus_type: &str,
) -> ConfigResult<hyperlane_base::settings::ChainConf> {
    let mut err = ConfigParsingError::default();
    println!("read chain {name} !");

    warn!(chain=%name, "read chain name");

    let domain = parse_domain(chain.clone(), name).take_config_err(&mut err);
    let signer = chain
        .chain(&mut err)
        .get_opt_key("signer")
        .and_then(parse_signer)
        .end();

    let reorg_period = chain
        .chain(&mut err)
        .get_opt_key("blocks")
        .get_key("reorgPeriod")
        .parse_u32()
        .unwrap_or(1);

    let rpcs = parse_base_and_override_urls(&chain, "rpcUrls", "customRpcUrls", "http", &mut err);

    let from = chain
        .chain(&mut err)
        .get_opt_key("index")
        .get_opt_key("from")
        .parse_u32()
        .unwrap_or(0);
    let chunk_size = chain
        .chain(&mut err)
        .get_opt_key("index")
        .get_opt_key("chunk")
        .parse_u32()
        .unwrap_or(1999);
    let mode = chain
        .chain(&mut err)
        .get_opt_key("index")
        .get_opt_key("mode")
        .parse_value("Invalid index mode")
        .unwrap_or_else(|| {
            domain
                .as_ref()
                .and_then(|d| match d.domain_protocol() {
                    HyperlaneDomainProtocol::Ethereum => Some(IndexMode::Block),
                    HyperlaneDomainProtocol::Sealevel => Some(IndexMode::Sequence),
                    _ => None,
                })
                .unwrap_or_default()
        });

    let mailbox = chain
        .chain(&mut err)
        .get_key("mailbox")
        .parse_address_hash()
        .end();
    let interchain_gas_paymaster = chain
        .chain(&mut err)
        .get_key("interchainGasPaymaster")
        .parse_address_hash()
        .end();
    let validator_announce = chain
        .chain(&mut err)
        .get_key("validatorAnnounce")
        .parse_address_hash()
        .end();
    let merkle_tree_hook = chain
        .chain(&mut err)
        .get_key("merkleTreeHook")
        .parse_address_hash()
        .end();

    let batch_contract_address = chain
        .chain(&mut err)
        .get_opt_key("batchContractAddress")
        .parse_address_hash()
        .end();

    let max_batch_size = chain
        .chain(&mut err)
        .get_opt_key("maxBatchSize")
        .parse_u32()
        .unwrap_or(1);

    cfg_unwrap_all!(&chain.cwp, err: [domain]);
    let connection = build_connection_conf(
        domain.domain_protocol(),
        &rpcs,
        &chain,
        &mut err,
        default_rpc_consensus_type,
        OperationBatchConfig {
            batch_contract_address,
            max_batch_size,
        },
    );

    cfg_unwrap_all!(&chain.cwp, err: [connection, mailbox, interchain_gas_paymaster, validator_announce, merkle_tree_hook]);
    err.into_result(hyperlane_base::settings::ChainConf {
        domain,
        signer,
        reorg_period,
        addresses: hyperlane_base::settings::CoreContractAddresses {
            mailbox,
            interchain_gas_paymaster,
            validator_announce,
            merkle_tree_hook,
        },
        connection,
        metrics_conf: Default::default(),
        index: hyperlane_base::settings::IndexSettings {
            from,
            chunk_size,
            mode,
        },
    })
}

/// Expects ChainMetadata
fn parse_domain(chain: ValueParser, name: &str) -> ConfigResult<HyperlaneDomain> {
    let mut err = ConfigParsingError::default();
    let internal_name = chain.chain(&mut err).get_key("name").parse_string().end();

    if let Some(internal_name) = internal_name {
        if internal_name != name {
            Err(eyre!(
                "detected chain name mismatch, the config may be corrupted"
            ))
        } else {
            Ok(())
        }
    } else {
        Err(eyre!("missing chain name, the config may be corrupted"))
    }
    .take_err(&mut err, || &chain.cwp + "name");

    let domain_id = chain
        .chain(&mut err)
        .get_opt_key("domainId")
        .parse_u32()
        .end()
        .or_else(|| chain.chain(&mut err).get_key("chainId").parse_u32().end());

    let protocol = chain
        .chain(&mut err)
        .get_key("protocol")
        .parse_from_str::<HyperlaneDomainProtocol>("Invalid Hyperlane domain protocol")
        .end();

    let technical_stack = chain
        .chain(&mut err)
        .get_opt_key("technicalStack")
        .parse_from_str::<hyperlane_core::HyperlaneDomainTechnicalStack>(
            "Invalid chain technical stack",
        )
        .end()
        .or_else(|| Some(hyperlane_core::HyperlaneDomainTechnicalStack::default()));

    cfg_unwrap_all!(&chain.cwp, err: [domain_id, protocol, technical_stack]);

    let domain = HyperlaneDomain::from_config(domain_id, name, protocol, technical_stack)
        .context("Invalid domain data")
        .take_err(&mut err, || chain.cwp.clone());

    cfg_unwrap_all!(&chain.cwp, err: [domain]);
    err.into_result(domain)
}

/// Expects AgentSigner.
fn parse_signer(signer: ValueParser) -> ConfigResult<SignerConf> {
    let mut err = ConfigParsingError::default();

    warn!("parsing signer");

    let signer_type = signer
        .chain(&mut err)
        .get_opt_key("type")
        .parse_string()
        .end();

    let key_is_some = matches!(signer.get_opt_key("key"), Ok(Some(_)));
    let id_is_some = matches!(signer.get_opt_key("id"), Ok(Some(_)));
    let region_is_some = matches!(signer.get_opt_key("region"), Ok(Some(_)));

    macro_rules! parse_signer {
        (hexKey) => {{
            let key = signer
                .chain(&mut err)
                .get_key("key")
                .parse_private_key()
                .unwrap_or_default();
            err.into_result(SignerConf::HexKey { key })
        }};
        (aws) => {{
            let id = signer
                .chain(&mut err)
                .get_key("id")
                .parse_string()
                .unwrap_or("")
                .to_owned();
            let region = signer
                .chain(&mut err)
                .get_key("region")
                .parse_from_str("Expected AWS region")
                .unwrap_or_default();
            err.into_result(SignerConf::Aws { id, region })
        }};
        (cosmosKey) => {{
            let key = signer
                .chain(&mut err)
                .get_key("key")
                .parse_private_key()
                .unwrap_or_default();
            let prefix = signer
                .chain(&mut err)
                .get_key("prefix")
                .parse_string()
                .unwrap_or_default();
            let account_address_type = signer
                .chain(&mut err)
                .get_opt_key("accountAddressType")
                .parse_from_str("Expected Account Address Type")
                .end()
                .unwrap_or_default();
            err.into_result(SignerConf::CosmosKey {
                key,
                prefix: prefix.to_string(),
                account_address_type,
            })
        }};
    }

    match signer_type {
        Some("hexKey") => parse_signer!(hexKey),
        Some("aws") => parse_signer!(aws),
        Some("cosmosKey") => parse_signer!(cosmosKey),
        Some(t) => {
            Err(eyre!("Unknown signer type `{t}`")).into_config_result(|| &signer.cwp + "type")
        }
        None if key_is_some => parse_signer!(hexKey),
        None if id_is_some | region_is_some => parse_signer!(aws),
        None => Ok(SignerConf::Node),
    }
}

fn parse_base_and_override_urls(
    chain: &ValueParser,
    base_key: &str,
    override_key: &str,
    protocol: &str,
    err: &mut ConfigParsingError,
) -> Vec<Url> {
    let base = parse_urls(chain, base_key, protocol, err);
    let overrides = parse_custom_urls(chain, override_key, err);
    let combined = overrides.unwrap_or(base);

    if combined.is_empty() {
        err.push(
            &chain.cwp + base_key,
            eyre!("Missing base {} definitions for chain", base_key),
        );
        err.push(
            &chain.cwp + "custom_rpc_urls",
            eyre!("Also missing {} overrides for chain", base_key),
        );
    }
    combined
}

fn parse_custom_urls(
    chain: &ValueParser,
    key: &str,
    err: &mut ConfigParsingError,
) -> Option<Vec<Url>> {
    chain
        .chain(err)
        .get_opt_key(key)
        .parse_string()
        .end()
        .map(|urls| {
            urls.split(',')
                .filter_map(|url| url.parse().take_err(err, || &chain.cwp + key))
                .collect_vec()
        })
}

fn parse_urls(
    chain: &ValueParser,
    key: &str,
    protocol: &str,
    err: &mut ConfigParsingError,
) -> Vec<Url> {
    chain
        .chain(err)
        .get_key(key)
        .into_array_iter()
        .map(|urls| {
            urls.filter_map(|v| {
                v.chain(err)
                    .get_key(protocol)
                    .parse_from_str("Invalid url")
                    .end()
            })
            .collect_vec()
        })
        .unwrap_or_default()
}

pub fn build_connection_conf(
    domain_protocol: HyperlaneDomainProtocol,
    rpcs: &[Url],
    chain: &ValueParser,
    err: &mut ConfigParsingError,
    default_rpc_consensus_type: &str,
    operation_batch: OperationBatchConfig,
) -> Option<hyperlane_base::settings::ChainConnectionConf> {
    match domain_protocol {
        HyperlaneDomainProtocol::Ethereum => build_ethereum_connection_conf(
            rpcs,
            chain,
            err,
            default_rpc_consensus_type,
            operation_batch,
        ),
        HyperlaneDomainProtocol::Fuel => rpcs.iter().next().map(|url| {
            ChainConnectionConf::Fuel(hyperlane_base::settings::parser::h_fuel::ConnectionConf {
                url: url.clone(),
            })
        }),
        HyperlaneDomainProtocol::Sealevel => rpcs.iter().next().map(|url| {
            ChainConnectionConf::Sealevel(
                hyperlane_base::settings::parser::h_sealevel::ConnectionConf {
                    url: url.clone(),
                    operation_batch,
                },
            )
        }),
        HyperlaneDomainProtocol::Cosmos => {
            build_cosmos_connection_conf(rpcs, chain, err, operation_batch)
        }
    }
}

pub fn build_cosmos_connection_conf(
    rpcs: &[Url],
    chain: &ValueParser,
    err: &mut ConfigParsingError,
    operation_batch: OperationBatchConfig,
) -> Option<ChainConnectionConf> {
    let mut local_err = ConfigParsingError::default();
    let grpcs =
        parse_base_and_override_urls(chain, "grpcUrls", "customGrpcUrls", "http", &mut local_err);

    let chain_id = chain
        .chain(&mut local_err)
        .get_key("chainId")
        .parse_string()
        .end()
        .or_else(|| {
            local_err.push(&chain.cwp + "chain_id", eyre!("Missing chain id for chain"));
            None
        });

    let prefix = chain
        .chain(err)
        .get_key("bech32Prefix")
        .parse_string()
        .end()
        .or_else(|| {
            local_err.push(
                &chain.cwp + "bech32Prefix",
                eyre!("Missing bech32 prefix for chain"),
            );
            None
        });

    let canonical_asset = if let Some(asset) = chain
        .chain(err)
        .get_opt_key("canonicalAsset")
        .parse_string()
        .end()
    {
        Some(asset.to_string())
    } else if let Some(hrp) = prefix {
        Some(format!("u{}", hrp))
    } else {
        local_err.push(
            &chain.cwp + "canonical_asset",
            eyre!("Missing canonical asset for chain"),
        );
        None
    };

    let gas_price = chain
        .chain(err)
        .get_opt_key("gasPrice")
        .and_then(parse_cosmos_gas_price)
        .end();

    let contract_address_bytes = chain
        .chain(err)
        .get_opt_key("contractAddressBytes")
        .parse_u64()
        .end();

    let native_token_decimals = chain
        .chain(err)
        .get_key("nativeToken")
        .get_key("decimals")
        .parse_u32()
        .unwrap_or(18);

    let native_token_denom = chain
        .chain(err)
        .get_key("nativeToken")
        .get_key("denom")
        .parse_string()
        .unwrap_or("");

    let native_token = NativeToken {
        decimals: native_token_decimals,
        denom: native_token_denom.to_owned(),
    };

    if !local_err.is_ok() {
        err.merge(local_err);
        None
    } else {
        Some(ChainConnectionConf::Cosmos(
            hyperlane_cosmos::ConnectionConf::new(
                grpcs,
                rpcs.first().unwrap().to_string(),
                chain_id.unwrap().to_string(),
                prefix.unwrap().to_string(),
                canonical_asset.unwrap(),
                gas_price.unwrap(),
                contract_address_bytes.unwrap().try_into().unwrap(),
                operation_batch,
                native_token,
            ),
        ))
    }
}

#[allow(clippy::question_mark)] // TODO: `rustc` 1.80.1 clippy issue
pub fn build_ethereum_connection_conf(
    rpcs: &[Url],
    chain: &ValueParser,
    err: &mut ConfigParsingError,
    default_rpc_consensus_type: &str,
    operation_batch: OperationBatchConfig,
) -> Option<hyperlane_base::settings::ChainConnectionConf> {
    let Some(first_url) = rpcs.to_owned().clone().into_iter().next() else {
        return None;
    };
    let rpc_consensus_type = chain
        .chain(err)
        .get_opt_key("rpcConsensusType")
        .parse_string()
        .unwrap_or(default_rpc_consensus_type);

    let rpc_connection_conf = match rpc_consensus_type {
        "single" => Some(RpcConnectionConf::Http { url: first_url }),
        "fallback" => Some(RpcConnectionConf::HttpFallback {
            urls: rpcs.to_owned().clone(),
        }),
        "quorum" => Some(RpcConnectionConf::HttpQuorum {
            urls: rpcs.to_owned().clone(),
        }),
        ty => Err(eyre!("unknown rpc consensus type `{ty}`"))
            .take_err(err, || &chain.cwp + "rpc_consensus_type"),
    };

    let transaction_overrides = chain
        .get_opt_key("transactionOverrides")
        .take_err(err, || &chain.cwp + "transaction_overrides")
        .flatten()
        .map(|value_parser| hyperlane_ethereum::TransactionOverrides {
            gas_price: value_parser
                .chain(err)
                .get_opt_key("gasPrice")
                .parse_u256()
                .end(),
            gas_limit: value_parser
                .chain(err)
                .get_opt_key("gasLimit")
                .parse_u256()
                .end(),
            max_fee_per_gas: value_parser
                .chain(err)
                .get_opt_key("maxFeePerGas")
                .parse_u256()
                .end(),
            max_priority_fee_per_gas: value_parser
                .chain(err)
                .get_opt_key("maxPriorityFeePerGas")
                .parse_u256()
                .end(),
        })
        .unwrap_or_default();

    Some(hyperlane_base::settings::ChainConnectionConf::Ethereum(
        ConnectionConf {
            rpc_connection: rpc_connection_conf?,
            transaction_overrides,
            operation_batch,
        },
    ))
}

/// Expects AgentSigner.
fn parse_cosmos_gas_price(gas_price: ValueParser) -> ConfigResult<RawCosmosAmount> {
    let mut err = ConfigParsingError::default();

    let amount = gas_price
        .chain(&mut err)
        .get_opt_key("amount")
        .parse_string()
        .end();

    let denom = gas_price
        .chain(&mut err)
        .get_opt_key("denom")
        .parse_string()
        .end();
    cfg_unwrap_all!(&gas_price.cwp, err: [denom, amount]);
    err.into_result(RawCosmosAmount::new(denom.to_owned(), amount.to_owned()))
}

/// Expects ValidatorAgentConfig.checkpointSyncer
fn parse_checkpoint_syncer(syncer: ValueParser) -> ConfigResult<CheckpointSyncerConf> {
    let mut err = ConfigParsingError::default();
    let syncer_type = syncer.chain(&mut err).get_key("type").parse_string().end();

    match syncer_type {
        Some("localStorage") => {
            let path = syncer
                .chain(&mut err)
                .get_key("path")
                .parse_from_str("Expected checkpoint syncer file path")
                .end();
            cfg_unwrap_all!(&syncer.cwp, err: [path]);
            err.into_result(CheckpointSyncerConf::LocalStorage { path })
        }
        Some("s3") => {
            let bucket = syncer
                .chain(&mut err)
                .get_key("bucket")
                .parse_string()
                .end()
                .map(str::to_owned);
            let region = syncer
                .chain(&mut err)
                .get_key("region")
                .parse_from_str("Expected aws region")
                .end();
            let folder = syncer
                .chain(&mut err)
                .get_opt_key("folder")
                .parse_string()
                .end()
                .map(str::to_owned);

            cfg_unwrap_all!(&syncer.cwp, err: [bucket, region]);
            err.into_result(CheckpointSyncerConf::S3 {
                bucket,
                region,
                folder,
            })
        }
        Some("gcs") => {
            let bucket = syncer
                .chain(&mut err)
                .get_key("bucket")
                .parse_string()
                .end()
                .map(str::to_owned);
            let folder = syncer
                .chain(&mut err)
                .get_opt_key("folder")
                .parse_string()
                .end()
                .map(str::to_owned);
            let service_account_key = syncer
                .chain(&mut err)
                .get_opt_key("service_account_key")
                .parse_string()
                .end()
                .map(str::to_owned);
            let user_secrets = syncer
                .chain(&mut err)
                .get_opt_key("user_secrets")
                .parse_string()
                .end()
                .map(str::to_owned);

            cfg_unwrap_all!(&syncer.cwp, err: [bucket]);
            err.into_result(CheckpointSyncerConf::Gcs {
                bucket,
                folder,
                service_account_key,
                user_secrets,
            })
        }
        Some(_) => {
            Err(eyre!("Unknown checkpoint syncer type")).into_config_result(|| &syncer.cwp + "type")
        }
        None => Err(err),
    }
}
