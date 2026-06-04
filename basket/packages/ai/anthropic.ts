import { parseSse } from "./sse.ts";
import type { ChatChunk, ChatRequest, ChatResponse, Provider } from "./types.ts";

export type AnthropicOptions = {
  readonly apiKey: string;
  readonly baseURL?: string;
  readonly defaultModel?: string;
  readonly version?: string;
};

const collectMessages = (req: ChatRequest): { system?: string; messages: unknown[] } => {
  const system: string[] = [];
  if (req.system) system.push(req.system);
  const msgs: unknown[] = [];
  for (const m of req.messages) {
    if (m.role === "system") system.push(m.content);
    else msgs.push({ role: m.role, content: m.content });
  }
  return { system: system.length > 0 ? system.join("\n\n") : undefined, messages: msgs };
};

export const anthropic = (options: AnthropicOptions): Provider => {
  const base = options.baseURL ?? "https://api.anthropic.com/v1";
  const model = options.defaultModel ?? "claude-sonnet-4-6";
  const version = options.version ?? "2023-06-01";

  const headers = (): Record<string, string> => ({
    "x-api-key": options.apiKey,
    "anthropic-version": version,
    "content-type": "application/json",
  });

  const chat = async (req: ChatRequest): Promise<ChatResponse> => {
    const { system, messages } = collectMessages(req);
    const res = await fetch(`${base}/messages`, {
      method: "POST",
      headers: headers(),
      signal: req.signal,
      body: JSON.stringify({
        model: req.model ?? model,
        max_tokens: req.maxTokens ?? 4096,
        temperature: req.temperature,
        system,
        messages,
      }),
    });
    if (!res.ok) throw new Error(`Anthropic chat failed: ${res.status} ${await res.text()}`);
    const data = (await res.json()) as {
      model: string;
      content: { type: string; text?: string }[];
      usage?: { input_tokens?: number; output_tokens?: number };
    };
    const content = data.content
      .filter((b) => b.type === "text")
      .map((b) => b.text ?? "")
      .join("");
    return {
      content,
      model: data.model,
      usage: data.usage
        ? { promptTokens: data.usage.input_tokens, completionTokens: data.usage.output_tokens }
        : undefined,
      raw: data,
    };
  };

  async function* chatStream(req: ChatRequest): AsyncIterable<ChatChunk> {
    const { system, messages } = collectMessages(req);
    const res = await fetch(`${base}/messages`, {
      method: "POST",
      headers: headers(),
      signal: req.signal,
      body: JSON.stringify({
        model: req.model ?? model,
        max_tokens: req.maxTokens ?? 4096,
        temperature: req.temperature,
        system,
        messages,
        stream: true,
      }),
    });
    if (!res.ok || !res.body) throw new Error(`Anthropic stream failed: ${res.status}`);

    for await (const event of parseSse(res.body)) {
      try {
        const parsed = JSON.parse(event) as {
          type: string;
          delta?: { type?: string; text?: string };
        };
        if (parsed.type === "content_block_delta" && parsed.delta?.type === "text_delta") {
          yield { delta: parsed.delta.text ?? "", done: false };
        }
        if (parsed.type === "message_stop") {
          yield { delta: "", done: true };
          return;
        }
      } catch {
        // partial json, skip
      }
    }
    yield { delta: "", done: true };
  }

  return { name: "anthropic", chat, chatStream };
};
