interface Plain { x: number }

export function inc(x: number): number {
  return x + 1;
}

export function die(code: number): number {
  process.exit(code);
}

export function nan(): number {
  return NaN;
}

export function big(): number {
  return 2 ** 60;
}

export function plain(n: number): Plain {
  return { x: n };
}

export function wrong(): unknown {
  return { x: "not a number" };
}

export function negative(): number {
  return -5;
}

export function noisy(): void {
  console.log("printed by TypeScript");
}

export function bigint(): bigint {
  return 1n;
}

export async function later(x: number): Promise<number> {
  await new Promise((r) => setTimeout(r, 5));
  return x * 2;
}

export function thrown(): number {
  throw new RangeError("out of range");
}
