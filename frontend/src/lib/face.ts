import { BufferAttribute, BufferGeometry, Euler, Mesh, Quaternion, Vector3 } from 'three';

/** Bake the asset orientation into both the neutral face and relative morph deltas. */
export function prepareFace(source: Mesh): { geometry: BufferGeometry; names: string[] } {
  const geometry = source.geometry.clone();
  source.updateWorldMatrix(true, false);
  const rotation = source.getWorldQuaternion(new Quaternion());
  const position = geometry.getAttribute('position');
  const center = new Vector3();
  const point = new Vector3();
  for (let i = 0; i < position.count; i++) {
    point.fromBufferAttribute(position, i).applyQuaternion(rotation);
    position.setXYZ(i, point.x, point.y, point.z);
    center.add(point);
  }
  center.divideScalar(position.count);
  for (let i = 0; i < position.count; i++) {
    point.fromBufferAttribute(position, i).sub(center);
    position.setXYZ(i, point.x, point.y, point.z);
  }
  const targets = geometry.morphAttributes.position ?? [];
  for (const target of targets) {
    for (let i = 0; i < target.count; i++) {
      point.fromBufferAttribute(target, i).applyQuaternion(rotation);
      if (!geometry.morphTargetsRelative) point.sub(center);
      target.setXYZ(i, point.x, point.y, point.z);
    }
  }
  const names = Object.entries(source.morphTargetDictionary ?? {}).sort((a, b) => a[1] - b[1]).map(([name]) => name);
  geometry.computeVertexNormals();
  return { geometry, names };
}

export function trackingWeight(values: Record<string, number>, name: string): number {
  const canonical = name[0].toUpperCase() + name.slice(1);
  const value = values[canonical] ?? values[name] ?? values[name.toLowerCase()] ?? 0;
  return Math.min(1, Math.max(0, value));
}

/** The same XYZ quaternion order and axis mapping as the existing Rust preview. */
export function headRotation(values: Record<string, number>): Quaternion {
  const angle = (value: number) => Math.min(1, Math.max(-1, value / 30)) * 0.95;
  return new Quaternion().setFromEuler(new Euler(
    angle(values.FaceAngleY ?? values.HeadRotY ?? values.headPitch ?? 0),
    angle(values.FaceAngleX ?? values.HeadRotX ?? values.headYaw ?? 0),
    angle(values.FaceAngleZ ?? values.HeadRotZ ?? values.headRoll ?? 0),
    'XYZ',
  ));
}

/** Recompute deformed normals so sparse position-only targets retain the studio lighting. */
export function deformFace(geometry: BufferGeometry, base: Float32Array, targets: BufferAttribute[], weights: number[]): void {
  const position = geometry.getAttribute('position') as BufferAttribute;
  const vertices = position.array as Float32Array;
  vertices.set(base);
  for (let t = 0; t < targets.length; t++) {
    const weight = weights[t];
    if (weight <= 0.0001) continue;
    const target = targets[t];
    for (let i = 0; i < position.count; i++) {
      for (let axis = 0; axis < 3; axis++) {
        const offset = i * 3 + axis;
        const delta = target.array[offset] - (geometry.morphTargetsRelative ? 0 : base[offset]);
        vertices[offset] += delta * weight;
      }
    }
  }
  position.needsUpdate = true;
  geometry.computeVertexNormals();
}
