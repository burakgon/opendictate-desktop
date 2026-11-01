import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { isMac } from "./lib/platform";
import "./styles.css";

// Platform-specific styling (macOS uses the native window frame and traffic lights).
if (isMac) document.documentElement.classList.add("platform-macos");

// Disable the browser context menu outside of text fields.
window.addEventListener("contextmenu", (e) => {
  const t = e.target as HTMLElement;
  if (!t.closest("input, textarea, .selectable")) e.preventDefault();
});

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
