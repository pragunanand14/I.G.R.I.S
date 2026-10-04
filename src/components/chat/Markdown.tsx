import { openUrl } from "@tauri-apps/plugin-opener";
import { Check, Copy } from "lucide-react";
import { memo, useState, type ReactNode } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import rehypeHighlight from "rehype-highlight";
import remarkGfm from "remark-gfm";
import { hasBackend } from "@/services/backend";
import "./markdown.css";

/** Text content of a React node tree (for copying highlighted code). */
function textOf(node: ReactNode): string {
  if (node == null || typeof node === "boolean") return "";
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(textOf).join("");
  if (typeof node === "object" && "props" in node) return textOf((node.props as { children?: ReactNode }).children);
  return "";
}

function CodeBlock({ children }: { children?: ReactNode }) {
  const [copied, setCopied] = useState(false);
  const child = Array.isArray(children) ? children[0] : children;
  const className = (child && typeof child === "object" && "props" in child ? (child.props as { className?: string }).className : "") ?? "";
  const lang = /language-([\w+#-]+)/.exec(className)?.[1];
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(textOf(children).replace(/\n$/, ""));
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      /* clipboard unavailable — leave the button state unchanged */
    }
  };
  return (
    <div className="md-code group">
      <div className="md-code__bar">
        <span>{lang ?? "text"}</span>
        <button type="button" onClick={() => void copy()} aria-label="Copy code" className="md-code__copy">
          {copied ? <Check className="size-3.5" /> : <Copy className="size-3.5" />}
          {copied ? "Copied" : "Copy"}
        </button>
      </div>
      <pre>{children}</pre>
    </div>
  );
}

const components: Components = {
  pre: ({ children }) => <CodeBlock>{children}</CodeBlock>,
  a: ({ href, children }) => (
    <a
      href={href}
      title={href}
      onClick={(e) => {
        // Never navigate the app window; open http(s) links in the system browser.
        e.preventDefault();
        if (href && /^https?:\/\//i.test(href) && hasBackend()) void openUrl(href);
      }}
    >
      {children}
    </a>
  ),
  table: ({ children }) => (
    <div className="md-table">
      <table>{children}</table>
    </div>
  ),
};

/**
 * Renders model output as Markdown. Raw HTML is not rendered (react-markdown's
 * default), so model or web content can't inject markup or scripts.
 */
export const Markdown = memo(function Markdown({ text }: { text: string }) {
  return (
    <div className="md" data-selectable>
      <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[[rehypeHighlight, { detect: false, ignoreMissing: true }]]} components={components}>
        {text}
      </ReactMarkdown>
    </div>
  );
});
