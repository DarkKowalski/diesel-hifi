/** Drawing inputs projected from the engine catalog at build time. */
export interface EngineGeometry {
  cylinders: number;
  stroke_m: number;
  connecting_rod_m: number;
  firing_order: number[];
}

export type EngineGeometries = Record<string, EngineGeometry>;
