//! Validator configuration.
//!
//! The correct settings shape is defined in the TypeScript SDK metadata. While the exact shape
//! and validations it defines are not applied here, we should mirror them.
//! ANY CHANGES HERE NEED TO BE REFLECTED IN THE TYPESCRIPT SDK.

use std::{collections::HashSet, path::PathBuf, time::Duration};

use derive_more::{AsMut, AsRef, Deref, DerefMut};
use eyre::{eyre, Context};
use hyperlane_base::{
    impl_loadable_from_settings,
    settings::{
        parser::{RawAgentConf, RawAgentSignerConf, ValueParser},
        CheckpointSyncerConf, Settings, SignerConf,
    },
};
use hyperlane_core::{cfg_unwrap_all, config::*, HyperlaneDomain};
use serde::Deserialize;
use serde_json::Value;

use std::default::Default;
/// Settings for `Validator`
#[derive(Debug, AsRef, AsMut, Deref, DerefMut)]
pub struct ValidatorSettings {
    #[as_ref]
    #[as_mut]
    #[deref]
    #[deref_mut]
    base: Settings,

    pub validators: Vec<SingleValidatorSettings>,
}

#[derive(Clone, Debug)]
pub struct SingleValidatorSettings {
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
        let mut err = ConfigParsingError::default();
        let p = ValueParser::new(cwp.clone(), &raw.0);

        // Parse the base config
        let base: Option<Settings> = p
            .parse_from_raw_config::<Settings, RawAgentConf, Option<&HashSet<&str>>>(
                None,
                "Expected valid base agent configuration",
            )
            .take_config_err(&mut err);
        let unwrapped_base = base.unwrap();

        // Collect value parsers for each single validator config
        let validator_parsers: Vec<ValueParser> = p
            .chain(&mut err)
            .get_key("validators")
            .into_array_iter()
            .unwrap()
            .collect();

        // Parse validator configs
        let mut validators: Vec<SingleValidatorSettings> = vec![];
        for parser in validator_parsers {
            let validator: SingleValidatorSettings =
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
) -> SingleValidatorSettings {
    let mut err = ConfigParsingError::default();

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

    let validator_settings: SingleValidatorSettings = SingleValidatorSettings {
        db: db.clone(),
        checkpoint_syncer: checkpoint_syncer.unwrap().clone(),
        interval,
        origin_chain: origin_chain.unwrap().clone(),
        validator: validator.unwrap().clone(),
        reorg_period,
    };

    return validator_settings;
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

            let aws_access_key_id = syncer
                .chain(&mut err)
                .get_key("awsAccessKeyId")
                .parse_string()
                .end()
                .map(str::to_owned);
            let aws_access_key_secret = syncer
                .chain(&mut err)
                .get_key("awsAccessKeySecret")
                .parse_string()
                .end()
                .map(str::to_owned);

            cfg_unwrap_all!(&syncer.cwp, err: [bucket, region, aws_access_key_id, aws_access_key_secret]);
            err.into_result(CheckpointSyncerConf::S3 {
                bucket,
                region,
                folder,
                aws_access_key_id,
                aws_access_key_secret,
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
