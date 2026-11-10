// Plugin authors may omit fields the existing Rust deserializer defaults.
export type OptionalWireFields<T, Keys extends keyof T> = Omit<T, Keys> & Partial<Pick<T, Keys>>;

export type OptionalWireCases<T, Keys extends PropertyKey> = T extends unknown ? OptionalWireFields<T, Extract<Keys, keyof T>> : never;
