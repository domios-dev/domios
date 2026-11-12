import type { HostIoRequest } from "./io";
import type { ExecPollResult } from "./gen/ExecPollResult";
import type { HostError } from "./gen/HostError";
import type { HostJobStart } from "./gen/HostJobStart";
import type { HostRefusal } from "./gen/HostRefusal";
import type { ReceiverTargetView } from "./gen/ReceiverTargetView";
import type { RecordingStatus } from "./gen/RecordingStatus";
import type { StreamPollResponse } from "./gen/StreamPollResponse";

type JobStart = Partial<HostJobStart & HostError>;

interface Results {
  execStart: JobStart;
  execPoll: ExecPollResult;
  execClose: void;
  fetchStart: JobStart;
  fetchStreamStart: JobStart;
  fetchStreamPoll: StreamPollResponse;
  fetchStreamClose: void;
  progress: string;
  localRecordingStatus: RecordingStatus | HostError | null;
  localRecordingSetEnabled: RecordingStatus | HostError | null;
  receiverStatusReport: number[] | HostError | null;
  receiverTarget: ReceiverTargetView | HostRefusal | null;
  receiverTargetClear: ReceiverTargetView | HostRefusal | null;
}

export type HostIoResult<Op extends keyof Results & HostIoRequest["op"]> = Results[Op];
