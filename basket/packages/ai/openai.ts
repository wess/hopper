import { parseSse } from "./sse.ts";
import type { ChatChunk, ChatRequest, ChatResponse, EmbedRequest, EmbedResponse, Provider } from "./types.ts";

export type OpenAiOptions = {
  readonly apiKey: string;
  readonly baseURL?: string;
  readonly defaultModel?: string;
  readonly defaultEmbedModel?: string;
};

const messages = (req: ChatRequest): unknown[] => {
  const out: unknown[] = [];
  if (req.system) out.push({ role: "system", content: req.system });
  for (const m of req.messages) out.push({ role: m.role, content: m.content });
  return out;
};

export const openai = (options: OpenAiOptions): Provider => {
  const base = options.baseURL ?? "https://api.openai.com/v1";
  const chatModel = options.defaultModel ?? "gpt-4o-mini";
  const embedModel = options.defaultEmbedModel ?? "text-embedding-3-small";

  const headers = (): Record<string, string> => ({
    authorization: `Bearer ${options.apiKey}`,
    "content-type": "application/json",
  });

  const chat = async (req: ChatRequest): Promise<ChatResponse> => {
    const res = await fetch(`${base}/chat/completions`, {
      method: "POST",
      headers: headers(),
      signal: req.signal,
      body: JSON.stringify({
        model: req.model ?? chatModel,
        messages: messages(req),
        temperature: req.temperature,
        max_tokens: req.maxTokens,
      }),
    });
    if (!res.ok) throw new Error(`OpenAI chat failed: ${res.status} ${await res.text()}`);
    const data = (await res.json()) as {
      model: string;
      choices: { message: { content: string } }[];
      usage?: { prompt_tokens?: number; completion_tokens?: number };
    };
    return {
      content: data.choices[0]?.message.content ?? "",
      model: data.model,
      usage: data.usage
        ? { promptTokens: data.usage.prompt_tokens, completionTokens: data.usage.completion_tokens }
        : undefined,
      raw: data,
    };
  };

  async function* chatStream(req: ChatRequest): AsyncIterable<ChatChunk> {
    const res = await fetch(`${base}/chat/completions`, {
      method: "POST",
      headers: headers(),
      signal: req.signal,
      body: JSON.stringify({
        model: req.model ?? chatModel,
        messages: messages(req),
        temperature: req.temperature,
        max_tokens: req.maxTokens,
        stream: true,
      }),
    });
    if (!res.ok || !res.body) throw new Error(`OpenAI stream failed: ${res.status}`);

    for await (const event of parseSse(res.body)) {
      if (event === "[DONE]") {
        yield { delta: "", done: true };
        return;
      }
      try {
        const parsed = JSON.parse(event) as { choices: { delta?: { content?: string } }[] };
        const delta = parsed.choices[0]?.delta?.content;
        if (delta) yield { delta, done: false };
      } catch {
        // partial json, skip
      }
    }
    yield { delta: "", done: true };
  }

  const embed = async (req: EmbedRequest): Promise<EmbedResponse> => {
    const res = await fetch(`${base}/embeddings`, {
      method: "POST",
      headers: headers(),
      body: JSON.stringify({ model: req.model ?? embedModel, input: req.input }),
    });
    if (!res.ok) throw new Error(`OpenAI embed failed: ${res.status} ${await res.text()}`);
    const data = (await res.json()) as { model: string; data: { embedding: number[] }[] };
    return { vectors: data.data.map((d) => d.embedding), model: data.model, raw: data };
  };

  return { name: "openai", chat, chatStream, embed };
};
