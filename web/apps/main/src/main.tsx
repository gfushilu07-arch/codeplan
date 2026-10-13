import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "@codeplan/design-tokens/tokens.css";
import "./global.css";
import { App } from "./App";

document.title = "CodePlan · 研发效能平台";

createRoot(document.getElementById("root") as HTMLElement).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
