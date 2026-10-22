export interface DevConnectionOptions {
  dataDir?: string;
  instance?: string;
  apiPort?: number;
}
export interface DevConnection {
  api: string;
  token: string;
  dataDir: string;
}
export function devConnection(options?: DevConnectionOptions): DevConnection;
