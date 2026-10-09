import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { BufferAttribute, Mesh, Vector3 } from 'three';
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';
import { deformFace, headRotation, prepareFace, trackingWeight } from './face';

const bytes = readFileSync(new URL('../../../resources/ARKitBlendshapeFaceMesh.glb', import.meta.url));
const gltf = await new GLTFLoader().parseAsync(bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength), '');
let source: Mesh | undefined;
gltf.scene.traverse((node) => { if ((node as Mesh).isMesh && !source) source = node as Mesh; });
if (!source) throw new Error('Face asset has no mesh');
const face = prepareFace(source);
const base = new Float32Array(face.geometry.getAttribute('position').array);
const targets = face.geometry.morphAttributes.position as BufferAttribute[];
const canonicalNames = readFileSync(new URL('../../../resources/tracking_data', import.meta.url), 'utf8').trim().split(/\r?\n/);

describe('existing ARKit face behavior', () => {
  it('loads the original topology and centered orientation', () => {
    expect(base.length / 3).toBe(6912);
    expect(face.geometry.index?.count).toBe(2304 * 3);
    expect(face.names).toHaveLength(51);
    const center = new Vector3();
    for (let i = 0; i < base.length; i += 3) center.add(new Vector3(base[i], base[i + 1], base[i + 2]));
    expect(center.length() / 6912).toBeLessThan(1e-7);
    // The original asset's +90° X transform must be baked exactly once.
    const raw = new Vector3().fromBufferAttribute(source!.geometry.getAttribute('position'), 0);
    const rotated = raw.clone().applyQuaternion(source!.quaternion);
    const rawCenter = new Vector3();
    const positions = source!.geometry.getAttribute('position');
    for (let i = 0; i < positions.count; i++) rawCenter.add(new Vector3().fromBufferAttribute(positions, i));
    rotated.sub(rawCenter.divideScalar(positions.count).applyQuaternion(source!.quaternion));
    expect(new Vector3(base[0], base[1], base[2]).distanceTo(rotated)).toBeLessThan(1e-7);
  });
  it('responds to every canonical tracking blendshape', () => {
    for (let index = 0; index < face.names.length; index++) {
      const name = face.names[index];
      const canonical = name[0].toUpperCase() + name.slice(1);
      expect(canonicalNames).toContain(canonical);
      const weights = face.names.map((target) => trackingWeight({ [canonical]: 1 }, target));
      deformFace(face.geometry, base, targets, weights);
      const posed = face.geometry.getAttribute('position').array;
      expect(posed.some((value, i) => Math.abs(value - base[i]) > 1e-7), canonical).toBe(true);
      expect(trackingWeight({ [name]: .5 }, name)).toBe(.5);
    }
  });
  it('combines morph targets and resets to neutral without accumulated drift', () => {
    const weights = face.names.map((name) => trackingWeight({ JawOpen: .8, EyeBlinkLeft: .5 }, name));
    deformFace(face.geometry, base, targets, weights);
    const first = new Float32Array(face.geometry.getAttribute('position').array);
    deformFace(face.geometry, base, targets, weights);
    expect(face.geometry.getAttribute('position').array).toEqual(first);
    deformFace(face.geometry, base, targets, weights.map(() => 0));
    expect(face.geometry.getAttribute('position').array).toEqual(base);
  });
  it('maps horizontal, vertical and tilt controls to the original axes', () => {
    const point = new Vector3(.04, .07, -.02);
    for (const [name, axis] of [['HeadRotX', 'y'], ['FaceAngleX', 'y'], ['HeadRotY', 'x'], ['FaceAngleY', 'x'], ['HeadRotZ', 'z']] as const) {
      const posed = point.clone().applyQuaternion(headRotation({ [name]: 15 }));
      expect(posed[axis]).toBeCloseTo(point[axis], 7);
      expect(posed.distanceTo(point)).toBeGreaterThan(.001);
    }
  });
  it('matches combined XYZ rotation and clamps extreme inputs', () => {
    const point = new Vector3(.04, .07, -.02);
    const expected = point.clone().applyAxisAngle(new Vector3(0, 0, 1), .1 / 30 * .95)
      .applyAxisAngle(new Vector3(0, 1, 0), .3 / 30 * .95).applyAxisAngle(new Vector3(1, 0, 0), .2 / 30 * .95);
    expect(point.clone().applyQuaternion(headRotation({ HeadRotX: .3, HeadRotY: .2, HeadRotZ: .1 })).distanceTo(expected)).toBeLessThan(1e-10);
    expect(headRotation({ HeadRotX: 90 })).toEqual(headRotation({ HeadRotX: 30 }));
    expect(trackingWeight({ JawOpen: 2 }, 'jawOpen')).toBe(1);
  });
});
