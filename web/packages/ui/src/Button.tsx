import type { CSSProperties, ButtonHTMLAttributes } from "react";

export type ButtonVariant = "primary" | "secondary" | "danger";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
}

const variantStyles: Record<ButtonVariant, CSSProperties> = {
  primary: {
    background: "var(--cp-color-primary)",
    color: "var(--cp-color-primary-fg)",
  },
  secondary: {
    background: "var(--cp-color-surface-alt)",
    color: "var(--cp-color-text)",
  },
  danger: {
    background: "var(--cp-color-danger)",
    color: "var(--cp-color-primary-fg)",
  },
};

/** 基础按钮(样式值全部来自 design-tokens,规范见 ARCHITECTURE.md §2.3)。 */
export function Button({ variant = "primary", style, ...rest }: ButtonProps) {
  return (
    <button
      {...rest}
      style={{
        border: "1px solid transparent",
        borderRadius: "var(--cp-radius-md)",
        padding: "var(--cp-space-2) var(--cp-space-3)",
        font: "inherit",
        cursor: "pointer",
        boxShadow: "var(--cp-shadow-1)",
        ...variantStyles[variant],
        ...style,
      }}
    />
  );
}
