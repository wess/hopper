import { BasketProvider } from "@basket/ui/provider";
import { createRoot } from "react-dom/client";
import { App } from "./app.tsx";

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <BasketProvider app={{ name: "Hopper", id: "io.wess.hopper" }} theme="dark">
      <App />
    </BasketProvider>,
  );
}
