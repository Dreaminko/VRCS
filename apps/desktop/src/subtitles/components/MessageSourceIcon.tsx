import { MessageSquare, Mic, Volume2 } from "lucide-react";

export type MessageSourceKind = "chatbox" | "microphone" | "voice";

export function messageSourceKind(source: string | null | undefined): MessageSourceKind {
  if (source === "chatbox" || source === "vrchat_chatbox") return "chatbox";
  if (source === "microphone") return "microphone";
  return "voice";
}

export function MessageSourceIcon({
  source,
  size = 14,
  className,
  chatboxLabel = "Chatbox",
  microphoneLabel,
  voiceLabel,
}: {
  source?: string | null;
  size?: number | string;
  className?: string;
  chatboxLabel?: string;
  microphoneLabel?: string;
  voiceLabel?: string;
}) {
  const kind = messageSourceKind(source);
  const shared = { size, className, "data-message-source": kind } as const;
  if (kind === "chatbox") return <MessageSquare {...shared} aria-label={chatboxLabel} />;
  if (kind === "microphone") return <Mic {...shared} aria-label={microphoneLabel} />;
  return <Volume2 {...shared} aria-label={voiceLabel} />;
}
