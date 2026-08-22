import { request } from "../core-client/transport";
import type { AnkiCardInput, AnkiStatus } from "./types";

export const ankiApi = {
  ankiStatus: () => request<AnkiStatus>("/api/anki/status"),
  createCard: (card: AnkiCardInput) => request<{ note_id: number }>("/api/anki/cards", {
    method: "POST",
    body: JSON.stringify(card),
  }),
};
