// Espelho de sona-core/src/config.rs. Mude os dois juntos.

export type SourceMap = { kind: 'none' } | { kind: 'mono'; input: number } | { kind: 'stereo'; left: number; right: number };
export type EqBandKind = 'peaking' | 'low_shelf' | 'high_shelf';
export type BusKind = 'broadcast' | 'pa_return' | 'recording' | 'monitor' | 'group';

export interface EqBand { enabled: boolean; kind: EqBandKind; freq_hz: number; gain_db: number; q: number }
export interface EqConfig { enabled: boolean; high_pass_enabled: boolean; high_pass_hz: number; bands: EqBand[] }
export interface GateConfig { enabled: boolean; threshold_db: number; range_db: number; attack_ms: number; hold_ms: number; release_ms: number }
export interface CompConfig { enabled: boolean; threshold_db: number; ratio: number; knee_db: number; attack_ms: number; release_ms: number; makeup_db: number }
export interface SendConfig { bus: number; enabled: boolean; level_db: number; pre_fader: boolean }

export interface ChannelConfig {
  name: string;
  /** Etiquetas livres ("Vocal", "Banda"…). */
  tags?: string[];
  source: SourceMap;
  trim_db: number;
  polarity_invert: boolean;
  gate: GateConfig;
  eq: EqConfig;
  comp: CompConfig;
  fader_db: number;
  mute: boolean;
  pan: number;
  sends: SendConfig[];
}

export interface OutputPair { left: number; right: number }
export interface MonitorConfig { follow: number | null; solo: number[]; solo_mode: 'pfl' | 'afl' }
export interface BusConfig {
  name: string;
  kind: BusKind;
  enabled: boolean;
  fader_db: number;
  mute: boolean;
  limiter: { enabled: boolean; ceiling_db: number; release_ms: number };
  outputs: OutputPair[];
  bus_sends: { bus: number; level_db: number }[];
  monitor: MonitorConfig | null;
}

export interface MixerConfig { version: number; channels: ChannelConfig[]; buses: BusConfig[] }
export interface Port { index: number; name: string; device: string; domain: number; present: boolean; connected: boolean }
export interface Issue { code: string; path: string; message: string }

export interface ServerState {
  rev: number;
  mixer: MixerConfig;
  scenes: string[];
  validation: { errors: Issue[]; warnings: Issue[] };
  io: { inputs: Port[]; outputs: Port[] };
}

export interface EngineStatus {
  device: string;
  running: boolean;
  sample_rate: number;
  block: number;
  load: number;
  load_peak: number;
  late_blocks: number;
  input_starved: number;
  blocks: number;
  clients: number;
  devices?: { role: string; name: string; label: string; connected: boolean; error: string | null; drift_ppm: number | null; latency_ms: number | null; underruns: number; overruns: number }[];
}

export const BUS_KIND_LABEL: Record<BusKind, string> = {
  broadcast: 'Transmissão',
  pa_return: 'Retorno PA',
  recording: 'Gravação',
  monitor: 'Fone',
  group: 'Grupo',
};
