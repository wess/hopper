// React error boundary. A view that throws during render would otherwise blank
// the entire app; this catches it and shows a recoverable fallback so the
// sidebar and the rest of the shell keep working. Error boundaries are the one
// place React still requires a class component — there is no hook equivalent for
// componentDidCatch.

import { Component, type ReactNode } from "react";
import { Button, EmptyState } from "./ui.tsx";

type Props = { readonly children: ReactNode };
type State = { error: Error | null };

export class ErrorBoundary extends Component<Props, State> {
  override state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  override componentDidCatch(error: Error): void {
    console.error("[Hopper] view crashed:", error);
  }

  override render(): ReactNode {
    const { error } = this.state;
    if (!error) return this.props.children;
    return (
      <EmptyState
        title="Something went wrong"
        hint={error.message}
        action={
          <Button variant="primary" onClick={() => this.setState({ error: null })}>
            Try again
          </Button>
        }
      />
    );
  }
}
