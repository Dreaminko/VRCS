export interface DictionaryEntry {
  term: string;
  language: string;
  definition: string;
  reading?: string | null;
  dictionary?: string | null;
}

export interface DictionarySource {
  id: number;
  title: string;
  revision: string;
  source_language: string;
  target_language: string | null;
  entry_count: number;
  imported_at: string;
}

export interface DictionaryImportProgress {
  progress: number;
}
