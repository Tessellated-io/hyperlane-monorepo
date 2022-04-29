import { MultisigValidatorManager } from '@abacus-network/core';
import { AbacusAppChecker, CheckerViolation } from '@abacus-network/deploy';
import {
  AbacusCore,
  ChainMap,
  ChainName,
  domains,
  MailboxAddresses,
  utils
} from '@abacus-network/sdk';
import { types } from '@abacus-network/utils';
import { expect } from 'chai';
import { setDifference } from '../utils/utils';
import { CoreConfig } from './types';

export enum CoreViolationType {
  ValidatorManager = 'ValidatorManager',
  Validator = 'Validator',
}

export enum ValidatorViolationType {
  EnrollValidator = 'EnrollValidator',
  UnenrollValidator = 'UnenrollValidator',
  Threshold = 'Threshold',
}

export interface ValidatorManagerViolation extends CheckerViolation {
  type: CoreViolationType.ValidatorManager;
}

export interface ValidatorViolation extends CheckerViolation {
  type: CoreViolationType.Validator;
  data: {
    type: ValidatorViolationType;
    validatorManagerAddress: string;
  };
}

export class AbacusCoreChecker<
  Networks extends ChainName,
> extends AbacusAppChecker<
  Networks,
  AbacusCore<Networks>,
  CoreConfig<Networks>
> {
  async checkOwners(owners: ChainMap<Networks, types.Address>) {
    return Promise.all(
      this.app
        .networks()
        .map((network) => this.checkDomain(network, owners[network])),
    );
  }

  async checkDomain(network: Networks, owner: types.Address): Promise<void> {
    await this.checkDomainOwnership(network, owner);
    await this.checkProxiedContracts(network);
    await this.checkOutbox(network);
    await this.checkInboxes(network);
    await this.checkAbacusConnectionManager(network);
    await this.checkValidatorManagers(network);
  }

  async checkDomainOwnership(
    network: Networks,
    owner: types.Address,
  ): Promise<void> {
    const contracts = this.app.getContracts(network);
    const ownables = [
      contracts.abacusConnectionManager,
      contracts.upgradeBeaconController,
      contracts.outbox.outbox,
      contracts.outbox.validatorManager,
      ...Object.values(contracts.inboxes)
        .map((inbox: any) => [inbox.inbox, inbox.validatorManager])
        .flat(),
    ];
    return this.checkOwnership(owner, ownables);
  }

  async checkOutbox(network: Networks): Promise<void> {
    const contracts = this.app.getContracts(network);
    const actualManager = await contracts.outbox.outbox.validatorManager();
    const expectedManager = contracts.outbox.validatorManager.address;
    if (actualManager !== expectedManager) {
      const violation: ValidatorManagerViolation = {
        network,
        type: CoreViolationType.ValidatorManager,
        actual: actualManager,
        expected: expectedManager,
      };
      this.addViolation(violation);
    }
  }

  // Checks validator sets of the OutboxValidatorManager and all
  // InboxValidatorManagers on the domain.
  async checkValidatorManagers(network: Networks) {
    const coreContracts = this.app.getContracts(network);
    await this.checkValidatorManager(
      network,
      network,
      coreContracts.outbox.validatorManager,
    );
    return utils.promiseObjAll<Record<any, void>>(
      utils.objMap(coreContracts.inboxes, (remote, inbox) =>
        this.checkValidatorManager(network, remote, inbox.validatorManager),
      ),
    );
  }

  // Checks the validator set for a MultisigValidatorManager on the localDomain that tracks
  // the validator set for the outboxDomain.
  // If localDomain == outboxDomain, this checks the OutboxValidatorManager, otherwise
  // it checks an InboxValidatorManager.
  async checkValidatorManager(
    local: Networks,
    remote: Networks,
    validatorManager: MultisigValidatorManager,
  ): Promise<void> {
    const validatorManagerConfig = this.config.validatorManagers[remote];
    const expectedValidators = validatorManagerConfig.validators;
    const actualValidators = await validatorManager.validators();

    const expectedSet = new Set<string>(expectedValidators);
    const actualSet = new Set<string>(actualValidators);

    const toEnroll = setDifference(expectedSet, actualSet);
    const toUnenroll = setDifference(actualSet, expectedSet);

    // Validators that should be enrolled
    for (const validatorToEnroll of toEnroll) {
      const violation: ValidatorViolation = {
        network: local,
        type: CoreViolationType.Validator,
        actual: undefined,
        expected: validatorToEnroll,
        data: {
          type: ValidatorViolationType.EnrollValidator,
          validatorManagerAddress: validatorManager.address,
        },
      };
      this.addViolation(violation);
    }

    // Validators that should be unenrolled
    for (const validatorToUnenroll of toUnenroll) {
      const violation: ValidatorViolation = {
        network: local,
        type: CoreViolationType.Validator,
        actual: validatorToUnenroll,
        expected: undefined,
        data: {
          type: ValidatorViolationType.UnenrollValidator,
          validatorManagerAddress: validatorManager.address,
        },
      };
      this.addViolation(violation);
    }

    const expectedThreshold = validatorManagerConfig.threshold;
    expect(expectedThreshold).to.not.be.undefined;

    const actualThreshold = await validatorManager.threshold();

    if (expectedThreshold !== actualThreshold.toNumber()) {
      const violation: ValidatorViolation = {
        network: local,
        type: CoreViolationType.Validator,
        actual: actualThreshold,
        expected: expectedThreshold,
        data: {
          type: ValidatorViolationType.Threshold,
          validatorManagerAddress: validatorManager.address,
        },
      };
      this.addViolation(violation);
    }
  }

  async checkInboxes(network: Networks): Promise<void> {
    const coreContracts = this.app.getContracts(network);

    // Check that all inboxes on this domain are pointed to the right validator
    // manager.
    await utils.promiseObjAll(
      utils.objMap(coreContracts.inboxes, async (_, inbox) => {
        const expected = inbox.validatorManager.address;
        const actual = await inbox.inbox.validatorManager();
        expect(actual).to.equal(expected);
      }),
    );

    // Check that all inboxes on this domain share the same implementation and
    // UpgradeBeacon.
    const coreAddresses = this.app.getAddresses(network);
    const inboxes: MailboxAddresses[] = Object.values(coreAddresses.inboxes);
    const implementations = inboxes.map((r) => r.implementation);
    const identical = (a: any, b: any) => (a === b ? a : false);
    const upgradeBeacons = inboxes.map((r) => r.beacon);
    expect(implementations.reduce(identical)).to.not.be.false;
    expect(upgradeBeacons.reduce(identical)).to.not.be.false;
  }

  async checkAbacusConnectionManager(domain: types.Domain): Promise<void> {
    const contracts = this.app.getContracts(domain);
    for (const remote of this.app.remoteDomainNumbers(domain)) {
      // inbox is enrolled in abacusConnectionManager
      const enrolledInboxes =
        await contracts.abacusConnectionManager.getInboxes(remote);
      expect(enrolledInboxes).to.eql([
        this.app.mustGetInbox(remote, domain).address,
      ]);
    }
    // Outbox is set on abacusConnectionManager
    const outbox = await contracts.abacusConnectionManager.outbox();
    expect(outbox).to.equal(contracts.outbox.address);
  }

    const outbox = await coreContracts.abacusConnectionManager.outbox();
    expect(outbox).to.equal(coreContracts.outbox.outbox.address);

    await utils.promiseObjAll(
      utils.objMap(coreContracts.inboxes, async (remote, inbox) => {
        const domain = domains[remote as ChainName].id;
        const enrolledInbox =
          await coreContracts.abacusConnectionManager.domainToInbox(domain);
        expect(enrolledInbox).to.equal(inbox.inbox.address);
      }),
    );
  }

  async checkProxiedContracts(network: Networks): Promise<void> {
    // Outbox upgrade setup contracts are defined
    const addresses = this.app.getAddresses(network);
    await this.checkUpgradeBeacon(network, 'Outbox', addresses.outbox);
    await utils.promiseObjAll(
      utils.objMap(addresses.inboxes, (network, inbox) =>
        this.checkUpgradeBeacon(network, 'Inbox', inbox),
      ),
    );
  }
}
