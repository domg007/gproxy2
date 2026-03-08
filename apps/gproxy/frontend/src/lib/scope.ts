import type { Scope } from "./types";

export function scopeAll<T>(): Scope<T> {
  return "All";
}

export function scopeEq<T>(value: T): Scope<T> {
  return { Eq: value };
}

export function scopeIn<T>(values: T[]): Scope<T> {
  return { In: values };
}
