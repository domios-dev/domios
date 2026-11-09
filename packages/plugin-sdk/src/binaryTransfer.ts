import type {OptionalWireCases, OptionalWireFields} from "./wireViews";
import type { MediaContentType } from "./mediaDelivery";

export type BinaryContentType = MediaContentType;
export type BinaryTransferDescriptor = OptionalWireFields<import("./gen/BinaryTransferDescriptor").BinaryTransferDescriptor, "maxUploadBytes" | "maxDownloadBytes" | "timeoutMs" | "credentialOrigins" | "requestHeaders">;
export type TransferCredential = OptionalWireCases<import("./gen/Credential").Credential, "prefix">;
export type TransferFileIdentity = import("./gen/FileIdentity").FileIdentity;
export type TransferInputGrant = import("./gen/TransferInputGrant").TransferInputGrant;
export type TransferInput = import("./gen/Input").Input;
export type TransferPart = import("./gen/Part").Part;
export type TransferRequest = Omit<OptionalWireFields<import("./gen/TransferRequest").TransferRequest, "headers" | "credential" | "body" | "timeoutMs">, "credential"> & {credential?: TransferCredential};
export type TransferRefusal = import("./gen/TransferRefusal").TransferRefusal;
export type TransferFailure = import("./gen/TransferFailure").TransferFailure;
export type TransferReceipt = import("./gen/TransferReceipt").TransferReceipt;
export type TransferResult = TransferReceipt | TransferFailure;
export type TransferStart = import("./gen/HostJobStart").HostJobStart | TransferFailure;
export interface HostTransfer {
  bindInput<T>(handle: string, destinationOrigin: string, then: (result: TransferInputGrant | TransferFailure) => T): T;
  start(request: TransferRequest): TransferStart;
  async<T>(request: TransferRequest, then: (result: TransferResult) => T): T;
  abort(jobId: string): boolean;
}
