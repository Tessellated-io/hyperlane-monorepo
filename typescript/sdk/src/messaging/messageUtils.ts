import { ethers } from 'ethers';
import { num, uint256 } from 'starknet';

import { ParsedMessage, ProtocolType } from '@hyperlane-xyz/utils';

import { DispatchedMessage } from '../core/types.js';

export function prepareMessageForRelay(
  message: DispatchedMessage,
  destinationProtocol: ProtocolType,
): {
  metadata: { size: number; data: bigint[] };
  messageData?: Uint8Array;
} {
  if (destinationProtocol === ProtocolType.Ethereum) {
    const ethMessage = toEthMessageBytes(
      message.parsed as ParsedMessage & {
        body: { size: bigint; data: bigint[] };
      },
    );

    return {
      metadata: { size: 0, data: [] },
      messageData: ethMessage,
    };
  }

  return {
    metadata: { size: 1, data: [BigInt(1)] },
  };
}

export function ethDispatchEventToStarkMessage(event: any): any {
  const messageBytes = Buffer.from(event.message.slice(2), 'hex');

  // Convert Buffer to BigNumberish using uint256.bnToUint256
  const sender = uint256.bnToUint256(
    '0x' + messageBytes.subarray(9, 41).toString('hex'),
  );
  const recipient = uint256.bnToUint256(
    '0x' + messageBytes.subarray(45, 77).toString('hex'),
  );

  // Parse message bytes
  const message = ethers.utils.arrayify(event.message);

  // Extract message components
  const version = message[0];

  // Extract nonce (4 bytes)
  const nonce = new DataView(message.slice(1, 5).buffer).getUint32(0, false);

  // Extract origin (4 bytes)
  const origin = new DataView(message.slice(5, 9).buffer).getUint32(0, false);

  // Extract body (skip first 77 bytes which contain header info)
  const body = message.slice(77);

  return {
    version,
    nonce: BigInt(nonce),
    origin: BigInt(origin),
    sender,
    destination: event.parsed.destination,
    recipient,
    body: toStarknetMessageBytes(body),
  };
}

export function toEthMessageBytes(
  starknetMessage: ParsedMessage & { body: { size: bigint; data: bigint[] } },
): Uint8Array {
  // Calculate buffer size based on Rust implementation
  const headerSize = 1 + 4 + 4 + 32 + 4 + 32; // version + nonce + origin + sender + destination + recipient
  const bodyBytes = u128VecToU8Vec(starknetMessage.body.data);

  // Create buffer with exact size needed
  const buffer = new Uint8Array(headerSize + bodyBytes.length);
  let offset = 0;

  // Write version (1 byte)
  buffer[offset] = Number(starknetMessage.version);
  offset += 1;

  // Write nonce (4 bytes)
  const view = new DataView(buffer.buffer);
  view.setUint32(offset, Number(starknetMessage.nonce), false); // false for big-endian
  offset += 4;

  // Write origin (4 bytes)
  view.setUint32(offset, Number(starknetMessage.origin), false);
  offset += 4;

  // Write sender (32 bytes)
  const senderValue =
    typeof starknetMessage.sender === 'string'
      ? BigInt(starknetMessage.sender)
      : starknetMessage.sender;
  const senderBytes = num.hexToBytes(num.toHex64(senderValue));
  buffer.set(senderBytes, offset);
  offset += 32;

  // Write destination (4 bytes)
  view.setUint32(offset, Number(starknetMessage.destination), false);
  offset += 4;

  // Write recipient (32 bytes)
  const recipientValue =
    typeof starknetMessage.recipient === 'string'
      ? BigInt(starknetMessage.recipient)
      : starknetMessage.recipient;
  const recipientBytes = num.hexToBytes(num.toHex64(recipientValue));
  buffer.set(recipientBytes, offset);
  offset += 32;

  // Write body
  buffer.set(bodyBytes, offset);

  return buffer;
}

/**
 * Convert a byte array to a starknet message
 * Pads the bytes to 16 bytes chunks
 * @param bytes Input byte array
 * @returns Object containing size and padded data array
 */
export function toStarknetMessageBytes(bytes: Uint8Array): {
  size: number;
  data: bigint[];
} {
  // Calculate the required padding
  const padding = (16 - (bytes.length % 16)) % 16;
  const totalLen = bytes.length + padding;

  // Create a new byte array with the necessary padding
  const paddedBytes = new Uint8Array(totalLen);
  paddedBytes.set(bytes);
  // Padding remains as zeros by default in Uint8Array

  // Convert to chunks of 16 bytes
  const result: bigint[] = [];
  for (let i = 0; i < totalLen; i += 16) {
    const chunk = paddedBytes.slice(i, i + 16);
    // Convert chunk to bigint (equivalent to u128 in Rust)
    const value = BigInt('0x' + Buffer.from(chunk).toString('hex'));
    result.push(value);
  }

  return {
    size: bytes.length,
    data: result,
  };
}

/**
 * Convert vector of u128 to bytes
 */
export function u128VecToU8Vec(input: bigint[]): Uint8Array {
  const output = new Uint8Array(input.length * 16); // Each u128 takes 16 bytes
  input.forEach((value, index) => {
    const hex = num.toHex(value);
    const bytes = num.hexToBytes(hex.padStart(34, '0')); // Ensure 16 bytes (34 chars including '0x')
    output.set(bytes, index * 16);
  });
  return output;
}
