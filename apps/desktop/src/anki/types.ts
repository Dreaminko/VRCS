export interface AnkiSettings {
  enabled: boolean;
  port: number;
  deck: string;
  model: string;
  front_field: string;
  back_field: string;
}

export interface AnkiStatus {
  connected: boolean;
  version: number | null;
  decks: string[];
  models: string[];
  fields: string[];
  configuration_valid: boolean;
  error_code: string | null;
  status_code: string;
  params: Record<string, unknown>;
  detail: string;
  message: string;
}

export interface AnkiCardInput {
  term: string;
  definition: string;
  context: string;
  reading?: string | null;
  dictionary?: string | null;
  language?: string | null;
  labels?: {
    definition: string;
    context: string;
  };
}
