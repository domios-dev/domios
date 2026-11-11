type CamelCaseKey<S extends string> = S extends `${infer Head}_${infer Tail}` ? `${Head}${Capitalize<CamelCaseKey<Tail>>}` : S;
type PairParameters = Omit<import("../../gen/OrchestrationPairParams").OrchestrationPairParams, "spawned_by_tab_id" | "leaf_creation_acknowledged">;
type PairInput = {[K in keyof PairParameters as null extends PairParameters[K] ? never : K]: PairParameters[K]} & {[K in keyof PairParameters as null extends PairParameters[K] ? K : never]?: NonNullable<PairParameters[K]>};
export type OrchestrationPairInput = {[K in keyof PairInput as CamelCaseKey<K>]: PairInput[K]};

export type OrchestrationPairResult = import("../../gen/OrchestrationPairResult").OrchestrationPairResult;
