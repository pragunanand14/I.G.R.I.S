import { Link } from "react-router";

export function NotFoundPage() {
  return (
    <div className="flex min-h-full flex-col items-center justify-center gap-3 p-8 text-center">
      <p className="text-label">404</p>
      <p className="text-sm text-muted">That page doesn't exist.</p>
      <Link to="/" className="text-xs text-accent hover:underline">
        Back to Home
      </Link>
    </div>
  );
}
