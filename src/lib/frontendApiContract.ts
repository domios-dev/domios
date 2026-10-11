import contract from "../../frontend-release-contract.json";

export const frontendApiContract = {
  version: contract.version,
  minApiVersion: contract.minApiVersion,
  maxApiVersion: contract.maxApiVersion,
};

export function frontendApiCompatible(apiVersion: unknown, range = frontendApiContract): apiVersion is number {
  return typeof apiVersion === "number" && Number.isSafeInteger(apiVersion)
    && Number.isSafeInteger(range.minApiVersion) && Number.isSafeInteger(range.maxApiVersion)
    && range.minApiVersion > 0 && range.minApiVersion <= range.maxApiVersion
    && apiVersion >= range.minApiVersion && apiVersion <= range.maxApiVersion;
}
