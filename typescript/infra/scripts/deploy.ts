import { providers } from 'ethers';
import path from 'path';

import {
  ChainMap,
  HyperlaneCoreDeployer,
  HyperlaneDeployer,
  HyperlaneIgp,
  HyperlaneIgpDeployer,
  HyperlaneIsmFactory,
  HyperlaneIsmFactoryDeployer,
  HyperlaneMessageHookDeployer,
  HyperlaneNoMetadataIsmDeployer,
  InterchainAccountDeployer,
  InterchainQueryDeployer,
  LiquidityLayerDeployer,
  MultiProvider,
  objMap,
} from '@hyperlane-xyz/sdk';

import { testConfigs } from '../config/environments/test/chains';
import { deployEnvToSdkEnv } from '../src/config/environment';
import { deployWithArtifacts } from '../src/deployment/deploy';
import { TestQuerySenderDeployer } from '../src/deployment/testcontracts/testquerysender';
import { TestRecipientDeployer } from '../src/deployment/testcontracts/testrecipient';
import { impersonateAccount, useLocalProvider } from '../src/utils/fork';
import { readJSON } from '../src/utils/utils';

import {
  Modules,
  SDK_MODULES,
  Sides,
  getArgs,
  getContractAddressesSdkFilepath,
  getEnvironmentConfig,
  getEnvironmentDirectory,
  getModuleDirectory,
  getRouterConfig,
  withModuleAndFork,
} from './utils';

// type Provider = providers.Provider;

async function main() {
  const { module, fork, environment, side } = await withModuleAndFork(getArgs())
    .argv;
  console.log('hooky: ', side);
  const envConfig = getEnvironmentConfig(environment);
  const multiProvider = await envConfig.getMultiProvider();

  if (fork) {
    await useLocalProvider(multiProvider, fork);

    // TODO: make this more generic
    const deployerAddress =
      environment === 'testnet3'
        ? '0xfaD1C94469700833717Fa8a3017278BC1cA8031C'
        : '0xa7ECcdb9Be08178f896c26b7BbD8C3D4E844d9Ba';

    const signer = await impersonateAccount(deployerAddress);
    multiProvider.setSigner(fork, signer);
  }

  let config: ChainMap<unknown> = {};
  let deployer: HyperlaneDeployer<any, any>;
  if (module === Modules.ISM_FACTORY) {
    config = objMap(envConfig.core, (chain) => true);
    deployer = new HyperlaneIsmFactoryDeployer(multiProvider);
  } else if (module === Modules.CORE) {
    config = envConfig.core;
    const ismFactory = HyperlaneIsmFactory.fromEnvironment(
      deployEnvToSdkEnv[environment],
      multiProvider,
    );
    console.log('config');
    console.log(config);
    console.log(multiProvider);
    deployer = new HyperlaneCoreDeployer(multiProvider, ismFactory);
  } else if (module === Modules.HOOK) {
    if (side == Sides.ISM) {
      config = {
        test2: {
          nativeType: 'ism',
          nativeBridge: '0x4200000000000000000000000000000000000007',
        },
      };

      deployer = new HyperlaneNoMetadataIsmDeployer(multiProvider);
    } else if (side == Sides.HOOK) {
      config = {
        test1: {
          nativeType: 'hook',
          nativeBridge: '0x25ace71c97B33Cc4729CF772ae268934F7ab5fA1',
          remoteIsm: '0x4c5859f0f772848b2d91f1d83e2fe57935348029',
          destinationDomain: 10,
        },
      };

      const newProvider = new providers.JsonRpcProvider(
        'http://localhost:8546',
      );

      // console.log("testConfigs: ", testConfigs);
      const test1Only = Object.fromEntries(
        Object.entries(testConfigs).filter(([k]) => k === 'test1'),
      );
      console.log('test1Only: ', test1Only);

      const newMultiProvider = new MultiProvider(testConfigs);
      newMultiProvider.setSigner(
        'test1',
        newProvider.getSigner('0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266'),
      );

      console.log('newProvider: ', newProvider);

      console.log(newMultiProvider);
      deployer = new HyperlaneMessageHookDeployer(newMultiProvider);
    } else {
      throw new Error('invalid side');
    }
  } else if (module === Modules.INTERCHAIN_GAS_PAYMASTER) {
    config = envConfig.igp;
    console.log('IGP: ', config);
    deployer = new HyperlaneIgpDeployer(multiProvider);
  } else if (module === Modules.INTERCHAIN_ACCOUNTS) {
    config = await getRouterConfig(environment, multiProvider);
    deployer = new InterchainAccountDeployer(multiProvider);
  } else if (module === Modules.INTERCHAIN_QUERY_SYSTEM) {
    config = await getRouterConfig(environment, multiProvider);
    deployer = new InterchainQueryDeployer(multiProvider);
  } else if (module === Modules.LIQUIDITY_LAYER) {
    const routerConfig = await getRouterConfig(environment, multiProvider);
    if (!envConfig.liquidityLayerConfig) {
      throw new Error(`No liquidity layer config for ${environment}`);
    }
    config = objMap(
      envConfig.liquidityLayerConfig.bridgeAdapters,
      (chain, conf) => ({
        ...conf,
        ...routerConfig[chain],
      }),
    );
    deployer = new LiquidityLayerDeployer(multiProvider);
  } else if (module === Modules.TEST_RECIPIENT) {
    console.log('test recipient');
    console.log(config);
    deployer = new TestRecipientDeployer(multiProvider);
  } else if (module === Modules.TEST_QUERY_SENDER) {
    // TODO: make this more generic
    const igp = HyperlaneIgp.fromEnvironment(
      deployEnvToSdkEnv[environment],
      multiProvider,
    );
    // Get query router addresses
    const queryRouterDir = path.join(
      getEnvironmentDirectory(environment),
      'middleware/queries',
    );
    config = objMap(readJSON(queryRouterDir, 'addresses.json'), (_c, conf) => ({
      queryRouterAddress: conf.router,
    }));
    deployer = new TestQuerySenderDeployer(multiProvider, igp);
  } else {
    console.log(`Skipping ${module}, deployer unimplemented`);
    return;
  }

  const modulePath = getModuleDirectory(environment, module);

  const addresses = SDK_MODULES.includes(module)
    ? path.join(
        getContractAddressesSdkFilepath(),
        `${deployEnvToSdkEnv[environment]}.json`,
      )
    : path.join(modulePath, 'addresses.json');

  const verification = path.join(modulePath, 'verification.json');

  const cache = {
    addresses,
    verification,
    read: environment !== 'test',
    write: true,
  };
  // Don't write agent config in fork tests
  const agentConfig =
    ['core', 'igp'].includes(module) && !fork
      ? {
          addresses,
          environment,
          multiProvider,
        }
      : undefined;
  console.log(
    'actual config: ',
    await await deployWithArtifacts(config, deployer, cache, fork, agentConfig),
  );
  await deployWithArtifacts(config, deployer, cache, fork, agentConfig);
}

main()
  .then()
  .catch((e) => {
    console.error(e);
    process.exit(1);
  });
