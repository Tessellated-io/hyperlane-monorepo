import { BigNumber, PopulatedTransaction } from 'ethers';
import { Contract, uint256 } from 'starknet';

import { Address, Domain, Numberish } from '@hyperlane-xyz/utils';

import { BaseStarknetAdapter } from '../../app/MultiProtocolApp.js';
import { MultiProtocolProvider } from '../../providers/MultiProtocolProvider.js';
import { ChainName } from '../../types.js';
import { TokenMetadata } from '../types.js';

import {
  InterchainGasQuote,
  TransferParams,
  TransferRemoteParams,
} from './ITokenAdapter.js';

export class StarknetNativeTokenAdapter extends BaseStarknetAdapter {
  async getBalance(address: Address): Promise<bigint> {
    // ETH ABI - we only need the balanceOf function
    const ethContract = new Contract(
      [
        {
          name: 'balanceOf',
          type: 'function',
          inputs: [{ name: 'account', type: 'felt' }],
          outputs: [{ name: 'balance', type: 'Uint256' }],
          stateMutability: 'view',
        },
      ],
      '0x049d36570d4e46f48e99674bd3fcc84644ddd6b96f7c741b1562b82f9e004dc7',
      this.getProvider(),
    );

    // Call balanceOf function
    const { balance } = await ethContract.balanceOf(address);

    return balance;
  }

  async getMetadata(): Promise<TokenMetadata> {
    // TODO get metadata from chainMetadata config
    throw new Error('Metadata not available to native tokens');
  }

  async getMinimumTransferAmount(_recipient: Address): Promise<bigint> {
    return 0n;
  }

  async isApproveRequired(
    _owner: Address,
    _spender: Address,
    _weiAmountOrId: Numberish,
  ): Promise<boolean> {
    return false;
  }

  async populateApproveTx(
    _params: TransferParams,
  ): Promise<PopulatedTransaction> {
    throw new Error('Approve not required for native tokens');
  }

  async populateTransferTx({
    weiAmountOrId,
    recipient,
  }: TransferParams): Promise<PopulatedTransaction> {
    const value = BigNumber.from(weiAmountOrId.toString());
    return { value, to: recipient };
  }

  async getTotalSupply(): Promise<bigint | undefined> {
    // Not implemented, native tokens don't have an accessible total supply
    return undefined;
  }
}

export class StarknetTokenAdapter extends StarknetNativeTokenAdapter {
  constructor(
    public readonly chainName: ChainName,
    public readonly multiProvider: MultiProtocolProvider,
    public readonly addresses: Record<string, Address>,
    public readonly denom: string,
  ) {
    super(chainName, multiProvider, addresses);
  }

  override async isApproveRequired(
    _owner: Address,
    _spender: Address,
    _weiAmountOrId: Numberish,
  ): Promise<boolean> {
    return false;
  }

  async quoteTransferRemoteGas(
    destination: Domain,
  ): Promise<InterchainGasQuote> {
    return { amount: BigInt(0) };
  }

  async populateTransferRemoteTx({
    weiAmountOrId,
    destination,
    recipient,
    interchainGas,
  }: TransferRemoteParams): Promise<PopulatedTransaction> {
    return { value: BigNumber.from(0) };
  }
}
export class StarknetHypSyntheticAdapter extends StarknetNativeTokenAdapter {
  constructor(
    public readonly chainName: ChainName,
    public readonly multiProvider: MultiProtocolProvider,
    public readonly addresses: Record<string, Address>,
    public readonly denom: string,
  ) {
    super(chainName, multiProvider, addresses);
  }

  override async isApproveRequired(
    _owner: Address,
    _spender: Address,
    _weiAmountOrId: Numberish,
  ): Promise<boolean> {
    return false;
  }

  async quoteTransferRemoteGas(
    destination: Domain,
  ): Promise<InterchainGasQuote> {
    return { amount: BigInt(0) };
  }

  async populateTransferRemoteTx({
    weiAmountOrId,
    destination,
    recipient,
    interchainGas,
  }: TransferRemoteParams): Promise<PopulatedTransaction> {
    // Create contract instance for the token
    const tokenContract = new Contract(
      [
        {
          name: 'transfer_remote',
          type: 'function',
          inputs: [
            { name: 'destination', type: 'felt' },
            { name: 'recipient', type: 'felt' },
            { name: 'amount', type: 'Uint256' },
          ],
          outputs: [],
        },
      ],
      this.addresses.token,
      this.getProvider(),
    );

    tokenContract.populateTransaction;

    // format evm address to starknet
    const recipientFelt = uint256.bnToUint256(recipient);

    // Prepare the transfer remote call
    const transferCall = {
      contractAddress: this.addresses.token,
      entrypoint: 'transfer_remote',
      calldata: [
        destination.toString(), // destination domain
        recipientFelt, // recipient address as felt
        weiAmountOrId.toString(), // amount
      ],
    };

    if (interchainGas?.amount && interchainGas.amount > 0n) {
      // For Starknet, we typically include the gas payment in the same transaction
      // by adding it to the transfer amount if it's the native token
      const totalAmount = BigInt(weiAmountOrId) + interchainGas.amount;
      transferCall.calldata[2] = totalAmount.toString();
    }

    return {
      ...transferCall,
      value: interchainGas?.amount
        ? BigNumber.from(interchainGas.amount)
        : undefined,
    };
  }
}
