import type { ButtonHTMLAttributes } from "react";

export type ButtonVariant = "primary" | "secondary" | "danger" | "ghost";
export type ButtonSize = "sm" | "md" | "lg";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
}

const VARIANT_CLASS: Record<ButtonVariant, string> = {
  primary: "cp-btn-primary",
  secondary: "",
  danger: "danger",
  ghost: "ghost",
};

const SIZE_CLASS: Record<ButtonSize, string> = {
  sm: "cp-btn-sm",
  md: "",
  lg: "cp-btn-lg",
};

/** 基础按钮:视觉全部来自全局 .cp-btn 组件类(见 web/apps/main/src/global.css)。 */
export function Button({ variant = "primary", size = "md", className, ...rest }: ButtonProps) {
  const cls = ["cp-btn", VARIANT_CLASS[variant], SIZE_CLASS[size], className]
    .filter(Boolean)
    .join(" ");
  return <button {...rest} className={cls} />;
}
