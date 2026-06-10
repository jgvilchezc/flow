import { readFileSync } from "node:fs";
const SYSTEM_PROMPT = readFileSync(
  new URL("../src-tauri/prompts/system_prompt.txt", import.meta.url),
  "utf8",
);
const FEW_SHOT = JSON.parse(
  readFileSync(new URL("../src-tauri/prompts/few_shot.json", import.meta.url), "utf8"),
);
// actual whisper-cli outputs from the benchmark clips
const cases = [
  ["es_corr", "Llegamos el viernes, no para, o mejor el sábado a las 10."],
  ["en_punct", "The meeting is at 5 p.m. period. Don't be late."],
  ["es_list", "Necesito que compres tres cosas, eh, leche, huevos, y fang."],
];
for (const model of ["gemma3:4b", "qwen2.5:7b"]) {
  console.log(`════ ${model}`);
  for (const [name, transcript] of cases) {
    const t0 = performance.now();
    const res = await fetch("http://localhost:11434/api/chat", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ model, stream: false, options: { temperature: 0.1 },
        messages: [
          { role: "system", content: SYSTEM_PROMPT },
          ...FEW_SHOT,
          { role: "user", content: transcript },
        ] }),
    });
    const body = await res.json();
    const ms = Math.round(performance.now() - t0);
    console.log(`── ${name} [${ms} ms]: ${(body.message?.content ?? "<ERR>").trim().replaceAll("\n", " ⏎ ")}`);
  }
}
