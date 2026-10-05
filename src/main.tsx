import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./styles/index.css";

const root = document.getElementById("root");
if (!root) throw new Error("Root element #root not found");

// The operator overlay windows load the same bundle with their own route and no app shell.
const overlay = window.location.hash.startsWith("#/operator/");

if (overlay) {
  void import("@/components/operator/OperatorOverlay").then(({ OperatorBorder, OperatorOrb }) => {
    const View = window.location.hash.startsWith("#/operator/border") ? OperatorBorder : OperatorOrb;
    createRoot(root).render(
      <StrictMode>
        <View />
      </StrictMode>,
    );
  });
} else {
  createRoot(root).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}
