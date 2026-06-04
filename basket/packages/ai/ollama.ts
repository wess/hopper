import type { ChatChunk, ChatRequest, ChatResponse, EmbedRequest, EmbedResponse, Provider } from "./types.ts";

export type OllamaOptions = {
  readonly baseURL?: string;
  readonly apiKey?: string;
  readonly defaultModel?: string;
  readonly defaultEmbedModel?: string;
};

const messages = (req: ChatRequest): unknown[] => {
  const out: unknown[] = [];
  if (req.system) out.push({ role: "system", content: req.system });
  for (const m of req.messages) out.push({ role: m.role, content: m.content });
  return out;
};

export const ollama = (options: OllamaOptions = {}): Provider => {
  const base = options.baseURL ?? "http://127.0.0.1:11434";
  const chatModel = options.defaultModel ?? "llama3.2";
  const embedModel = options.defaultEmbedModel ?? "nomic-embed-text";

  // Ollama Cloud (and any authenticated proxy) needs a bearer token; a
  // local `ollama serve` simply ignores it.
  const headers = (): Record<string, string> => {
    const h: Record<string, string> = { "content-type": "application/json" };
    if (options.apiKey) h.authorization = `Bearer ${options.apiKey}`;
    return h;
  };

  const chat = async (req: ChatRequest): Promise<ChatResponse> => {
    const res = await fetch(`${base}/api/chat`, {
      method: "POST",
      headers: headers(),
      signal: req.signal,
      body: JSON.stringify({
        model: req.model ?? chatModel,
        messages: messages(req),
        stream: false,
        options: { temperature: req.temperature, num_predict: req.maxTokens },
      }),
    });
    if (!res.ok) throw new Error(`Ollama chat failed: ${res.status} ${await res.text()}`);
    const data = (await res.json()) as { model: string; message: { content: string } };
    return { content: data.message.content, model: data.model, raw: data };
  };

  async function* chatStream(req: ChatRequest): AsyncIterable<ChatChunk> {
    const res = await fetch(`${base}/api/chat`, {
      method: "POST",
      headers: headers(),
      signal: req.signal,
      body: JSON.stringify({
        model: req.model ?? chatModel,
        messages: messages(req),
        stream: true,
        options: { temperature: req.temperature, num_predict: req.maxTokens },
      }),
    });
    if (!res.ok || !res.body) throw new Error(`Ollama stream failed: ${res.status}`);

    const reader = res.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";
    while (true) {
      const { value, done } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });
      const lines = buffer.split("\n");
      buffer = lines.pop() ?? "";
      for (const line of lines) {
        if (!line.trim()) continue;
        try {
          const parsed = JSON.parse(line) as { message?: { content?: string }; done?: boolean };
          if (parsed.message?.content) yield { delta: parsed.message.content, done: false };
          if (parsed.done) {
            yield { delta: "", done: true };
            return;
          }
        } catch {
          // skip partial
        }
      }
    }
    yield { delta: "", done: true };
  }

  const embed = async (req: EmbedRequest): Promise<EmbedResponse> => {
    const inputs = Array.isArray(req.input) ? req.input : [req.input];
    const vectors: number[][] = [];
    for (const input of inputs) {
      const res = await fetch(`${base}/api/embeddings`, {
        method: "POST",
        headers: headers(),
        body: JSON.stringify({ model: req.model ?? embedModel, prompt: input }),
      });
      if (!res.ok) throw new Error(`Ollama embed failed: ${res.status}`);
      const data = (await res.json()) as { embedding: number[] };
      vectors.push(data.embedding);
    }
    return { vectors, model: req.model ?? embedModel, raw: { count: vectors.length } };
  };

  return { name: "ollama", chat, chatStream, embed };
};
