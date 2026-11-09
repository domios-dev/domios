export type ProviderAuthStatus = import("./gen/AuthStatus").AuthStatus;

export type RuntimeErrorSignal = Pick<import("./gen/RuntimeErrorSignal").RuntimeErrorSignal, "retryable"> & Partial<{[K in "retryableKind" | "retryAtMs"]: NonNullable<import("./gen/RuntimeErrorSignal").RuntimeErrorSignal[K]>}> & {code?: string; class?: string};

export type ProviderSessionObservations = Omit<import("./gen/SessionObservations").SessionObservations, "retrying" | "retry_error"> & {retrying?: import("./gen/SessionObservations").SessionObservations["retrying"]; retry_error?: RuntimeErrorSignal};
