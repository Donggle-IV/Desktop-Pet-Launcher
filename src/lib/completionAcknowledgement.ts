export type CompletionAcknowledgementMode = "one-cycle" | "pending-focus";

export interface CompletionAcknowledgement {
  mode: CompletionAcknowledgementMode;
  token: number;
}

export function completionAcknowledgementForFocus(
  token: number,
  focused: boolean,
): CompletionAcknowledgement {
  return { token, mode: focused ? "one-cycle" : "pending-focus" };
}

export function shouldClearOneCycleAcknowledgement(
  current: CompletionAcknowledgement | null,
  token: number,
): boolean {
  return current?.mode === "one-cycle" && current.token === token;
}

export function shouldConsumePendingAcknowledgement(
  current: CompletionAcknowledgement | null,
  focused: boolean,
): boolean {
  return focused && current?.mode === "pending-focus";
}
