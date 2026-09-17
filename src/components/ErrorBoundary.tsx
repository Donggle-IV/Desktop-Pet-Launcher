import { Component, type ErrorInfo, type ReactNode } from "react";

interface Props {
  children: ReactNode;
  fallbackTitle?: string;
}

interface State {
  hasError: boolean;
  message: string;
}

/**
 * Catches render errors so a single component crash does not blank the entire
 * transparent pet / settings window.
 */
export class ErrorBoundary extends Component<Props, State> {
  state: State = {
    hasError: false,
    message: "",
  };

  static getDerivedStateFromError(error: unknown): State {
    const message =
      error instanceof Error ? error.message : typeof error === "string" ? error : "Unknown error";
    return { hasError: true, message };
  }

  componentDidCatch(error: unknown, info: ErrorInfo): void {
    console.error("Desktop Pet UI crashed", error, info.componentStack);
  }

  private handleReload = () => {
    this.setState({ hasError: false, message: "" });
    window.location.reload();
  };

  render() {
    if (!this.state.hasError) {
      return this.props.children;
    }

    return (
      <div
        style={{
          boxSizing: "border-box",
          minWidth: 220,
          minHeight: 160,
          padding: 16,
          margin: 8,
          borderRadius: 12,
          background: "rgba(20, 24, 33, 0.92)",
          color: "#f5f7ff",
          fontFamily: "system-ui, sans-serif",
          fontSize: 13,
          lineHeight: 1.45,
          boxShadow: "0 8px 24px rgba(0,0,0,0.35)",
        }}
      >
        <strong style={{ display: "block", marginBottom: 8 }}>
          {this.props.fallbackTitle ?? "데스크톱 펫 화면 오류"}
        </strong>
        <p style={{ margin: "0 0 12px", opacity: 0.85, wordBreak: "break-word" }}>
          {this.state.message || "처리되지 않은 화면 렌더링 오류가 발생했습니다."}
        </p>
        <button
          type="button"
          onClick={this.handleReload}
          style={{
            border: "none",
            borderRadius: 8,
            padding: "8px 12px",
            background: "#6c8cff",
            color: "#fff",
            cursor: "pointer",
          }}
        >
          다시 불러오기
        </button>
      </div>
    );
  }
}
