/** Small display helpers. Presentation only; no unit conversion in the core. */

export function rpm(value: number): string {
  return value.toFixed(0);
}

export function bar(pascals: number): string {
  return (pascals / 1e5).toFixed(1);
}

export function mpa(pascals: number): string {
  return (pascals / 1e6).toFixed(2);
}

export function nm(value: number): string {
  return value.toFixed(0);
}

export function mg(value: number): string {
  return value.toFixed(1);
}

export function seconds(value: number): string {
  return value.toFixed(2);
}

export function degrees(radians: number): string {
  return ((radians * 180) / Math.PI).toFixed(0);
}

export function kelvin(value: number): string {
  return value.toFixed(0);
}
