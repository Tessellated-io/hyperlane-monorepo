import { BigNumber } from 'ethers';
import { CairoOption, CairoOptionVariant, Call } from 'starknet';

import { Address, Domain } from '@hyperlane-xyz/utils';

import { BaseStarknetAdapter } from '../../app/MultiProtocolApp.js';
import { MultiProtocolProvider } from '../../providers/MultiProtocolProvider.js';
import { ChainName } from '../../types.js';
import { getStarknetHypERC20Contract } from '../../utils/starknet.js';
import { TokenMetadata } from '../types.js';

import {
  IHypTokenAdapter,
  InterchainGasQuote,
  TransferParams,
  TransferRemoteParams,
} from './ITokenAdapter.js';

const ETH_ADDRESS =
  '0x049d36570d4e46f48e99674bd3fcc84644ddd6b96f7c741b1562b82f9e004dc7';

export class StarknetNativeTokenAdapter extends BaseStarknetAdapter {
  async getBalance(address: Address): Promise<bigint> {
    // On starknet, native tokens are ERC20s
    const tokenContract = await this.getERC20Contract(ETH_ADDRESS);
    const res = await tokenContract.balanceOf(address);
    return res;
  }

  async getMetadata(): Promise<TokenMetadata> {
    return {
      symbol: 'ETH',
      name: 'Ethereum',
      totalSupply: 0,
      decimals: 18,
    };
  }

  async getMinimumTransferAmount(_recipient: Address): Promise<bigint> {
    return 0n;
  }

  async isApproveRequired(): Promise<boolean> {
    return false;
  }

  async populateApproveTx(_params: TransferParams): Promise<Call> {
    throw new Error('Approve not required for native tokens'); // TODO: double check for starknet
  }

  async populateTransferTx({
    weiAmountOrId,
    recipient,
  }: TransferParams): Promise<Call> {
    const tokenContract = await this.getERC20Contract(ETH_ADDRESS);
    return tokenContract.populateTransaction.transfer(recipient, weiAmountOrId);
  }

  async getTotalSupply(): Promise<bigint | undefined> {
    return undefined;
  }
}

export class StarknetHypSyntheticAdapter
  extends StarknetNativeTokenAdapter
  implements IHypTokenAdapter<Call>
{
  constructor(
    public readonly chainName: ChainName,
    public readonly multiProvider: MultiProtocolProvider,
    public readonly addresses: { token: Address },
  ) {
    super(chainName, multiProvider, addresses);
  }

  override async getBalance(address: Address): Promise<bigint> {
    const tokenContract = await this.getERC20Contract(this.addresses.token);
    return tokenContract.balanceOf(address);
  }

  override async populateTransferTx({
    weiAmountOrId,
    recipient,
  }: TransferParams): Promise<Call> {
    const tokenContract = await this.getERC20Contract(this.addresses.token);
    return tokenContract.populateTransaction.transfer(recipient, weiAmountOrId);
  }

  async quoteTransferRemoteGas(
    _destination: Domain,
  ): Promise<InterchainGasQuote> {
    return { amount: BigInt(0) };
  }

  async populateTransferRemoteTx({
    weiAmountOrId,
    destination,
    recipient,
    interchainGas,
  }: TransferRemoteParams): Promise<Call> {
    const hypToken = getStarknetHypERC20Contract(this.addresses.token);
    const nonOption = new CairoOption(CairoOptionVariant.None);

    const transferTx = hypToken.populateTransaction.transfer_remote(
      destination,
      recipient,
      BigInt(weiAmountOrId.toString()),
      BigInt(0),
      nonOption,
      nonOption,
    );

    // TODO: add gas payment when we support it

    return {
      ...transferTx,
      value: interchainGas?.amount
        ? BigNumber.from(interchainGas.amount)
        : BigNumber.from(0),
    };
  }

  async getDomains(): Promise<Domain[]> {
    return [];
  }

  async getRouterAddress(domain: Domain): Promise<Buffer> {
    return Buffer.from(this.addresses.token);
  }

  async getAllRouters(): Promise<Array<{ domain: Domain; address: Buffer }>> {
    return [];
  }

  async getBridgedSupply(): Promise<bigint | undefined> {
    return undefined;
  }
}
