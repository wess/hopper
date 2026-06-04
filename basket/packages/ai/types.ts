export type Role = "system" | "user" | "assistant";

export type Message = {
  readonly role: Role;
  readonly content: string;
};

export type ChatRequest = {
  readonly messages: readonly Message[];
  readonly model?: string;
  readonly temperature?: number;
  readonly maxTokens?: number;
  readonly system?: string;
  readonly signal?: AbortSignal;
};

export type ChatResponse = {
  readonly content: string;
  readonly model: string;
  readonly usage?: { promptTokens?: number; completionTokens?: number };
  readonly raw: unknown;
};

export type ChatChunk = {
  readonly delta: string;
  readonly done: boolean;
};

export type EmbedRequest = {
  readonly input: string | readonly string[];
  readonly model?: string;
};

export type EmbedResponse = {
  readonly vectors: readonly (readonly number[])[];
  readonly model: string;
  readonly raw: unknown;
};

export type Provider = {
  readonly name: string;
  readonly chat: (req: ChatRequest) => Promise<ChatResponse>;
  readonly chatStream: (req: ChatRequest) => AsyncIterable<ChatChunk>;
  readonly embed?: (req: EmbedRequest) => Promise<EmbedResponse>;
};
