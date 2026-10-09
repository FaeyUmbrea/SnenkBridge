export interface DelaySettings {
  refParam: string; smoothing: number; delayCount: number;
  inMin: number; inMax: number; outMin: number; outMax: number;
}
export interface Parameter {
  name: string; func: string; min: number; max: number; defaultValue: number;
  delayBuffer?: DelaySettings;
}
export interface Preset {
  format: string; version: number; title: string; author?: string; description?: string;
  params: Parameter[];
}
export interface Settings {
  preset_name: string; phone_ip: string; tracking_type_index: number;
  face_search_timeout: string; vts_ip: string; vts_port: string;
}
export interface Preview {
  inputs: Record<string, number>; outputs: Record<string, number>;
  source_active: boolean; target_active: boolean;
  source_status: string; target_status: string; face_present: boolean;
}
export interface Snapshot {
  preset: Preset; choices: string[]; selected: number; can_delete: boolean; dirty: boolean;
  settings: Settings; errors: string[]; error: string; preview: Preview;
  variables: string[]; credits: string;
}
