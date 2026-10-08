/** Adds. */
export function add(a: number, b: number): number {
  return a + b;
}

export const mul = (a: number, b: number): number => a * b;

export interface Shape {
  area(): number;
}

export type Pair = [number, number];

export enum Color {
  Red,
  Green,
}

export class Circle implements Shape {
  constructor(private r: number) {}
  area(): number {
    return Math.PI * this.r * this.r;
  }
  static unit(): Circle {
    return new Circle(1);
  }
}
