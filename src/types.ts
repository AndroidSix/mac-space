export interface ProcessInfo {
  pid: number;
  name: string;
  cpu: number;
  memory: number;
  startTime: number;
  manageable: boolean;
  nice: number;
}
export interface Metrics {
  cpu: number;
  cores: number;
  memoryUsed: number;
  memoryTotal: number;
  swapUsed: number;
  diskTotal: number;
  diskAvailable: number;
  thermalState: number;
  lowPower: boolean;
  sensors: { name: string; celsius: number }[];
  processes: ProcessInfo[];
  processCount: number;
  loadAverage: number[];
  sampledAt: number;
  chip: string;
  os: string;
}
export interface Entry {
  id: string;
  name: string;
  path: string;
  group: string;
  bytes: number;
  files: number;
  errors: number;
  blocked: string[];
  device: number;
  inode: number;
  isDirectory: boolean;
}
export interface DirectoryScan {
  root: string;
  entries: Entry[];
  scannedAt: number;
  files: number;
  errors: number;
  skippedMounts: number;
  errorPaths: string[];
  cancelled: boolean;
}
export interface Snapshot {
  caches: Entry[];
  cacheScannedAt: number;
  directory: DirectoryScan | null;
  dirty: boolean;
  watching?: boolean;
}
export interface Progress {
  path: string;
  files: number;
  bytes: number;
  errors: number;
}
export type Language = 'system' | 'zh' | 'en';
export interface ApiConfig {
  enabled: boolean;
  port: number;
  token: string;
}
export interface UpdateInfo {
  current: string;
  latest: string;
  available: boolean;
  url: string;
  checkedAt: number;
}
export interface CleanupProgress {
  id: string;
  completed: number;
  total: number;
}
export interface BatchReport {
  items: { id: string; error: string | null }[];
  cancelled: boolean;
  availableChange: number;
  persistenceError: string | null;
}
