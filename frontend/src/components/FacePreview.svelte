<script lang="ts">
  import { onMount } from 'svelte';
  import { BufferAttribute, Mesh, PerspectiveCamera, Scene, ShaderMaterial, WebGLRenderer } from 'three';
  import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';
  import { deformFace, headRotation, prepareFace, trackingWeight } from '../lib/face';
  import faceUrl from '../../../resources/ARKitBlendshapeFaceMesh.glb?url';

  let { values }: { values: Record<string, number> } = $props();
  let host: HTMLDivElement;
  let canvas: HTMLCanvasElement;
  let error = $state('');
  let update: (() => void) | undefined;
  $effect(() => { void values; update?.(); });

  onMount(() => {
    let renderer: WebGLRenderer;
    try { renderer = new WebGLRenderer({ canvas, antialias: true, alpha: true, powerPreference: 'low-power' }); }
    catch (e) { error = `Could not start the 3D preview: ${String(e)}`; return; }
    renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    const scene = new Scene();
    const camera = new PerspectiveCamera(28.764, 1, 0.01, 2);
    camera.position.z = 0.48;
    const material = new ShaderMaterial({
      vertexShader: `varying vec3 faceNormal;
        void main() {
          faceNormal = normalMatrix * normal;
          gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
        }`,
      fragmentShader: `varying vec3 faceNormal;
        void main() {
          vec3 n = normalize(faceNormal);
          vec3 key = normalize(vec3(0.35, 0.55, 0.75));
          vec3 fill = normalize(vec3(-0.45, -0.2, 0.6));
          float diffuseKey = max(dot(n, key), 0.0);
          float diffuseFill = max(dot(n, fill), 0.0);
          float specular = pow(max(dot(n, normalize(key + vec3(0.0, 0.0, 1.0))), 0.0), 14.0);
          float rim = pow(1.0 - clamp(n.z, 0.0, 1.0), 2.0);
          vec3 color = (vec3(36.0, 34.0, 44.0) + diffuseKey * vec3(170.0, 155.0, 175.0)
            + diffuseFill * vec3(55.0, 70.0, 95.0) + specular * vec3(140.0, 140.0, 160.0)
            + rim * vec3(35.0, 30.0, 45.0)) / 255.0;
          gl_FragColor = vec4(clamp(color, 0.0, 1.0), 1.0);
        }`,
    });
    let mesh: Mesh | undefined;
    let disposed = false;
    let animation = 0;
    const render = () => {
      if (animation || disposed) return;
      animation = requestAnimationFrame(() => { animation = 0; renderer.render(scene, camera); });
    };
    const resize = new ResizeObserver(() => {
      const size = Math.min(host.clientWidth, host.clientHeight);
      if (size > 0) { renderer.setSize(size, size); render(); }
    });
    resize.observe(host);
    const contextLost = (event: Event) => { event.preventDefault(); error = '3D preview lost its graphics context. Reopen the Preview tab to reload it.'; };
    renderer.domElement.addEventListener('webglcontextlost', contextLost);
    new GLTFLoader().load(faceUrl, (gltf) => {
      let source: Mesh | undefined;
      gltf.scene.traverse((node) => { if ((node as Mesh).isMesh && !source) source = node as Mesh; });
      if (!source) { error = 'The face asset contains no mesh.'; return; }
      const { geometry, names } = prepareFace(source);
      gltf.scene.traverse((node) => {
        if ((node as Mesh).isMesh) {
          const assetMesh = node as Mesh;
          assetMesh.geometry.dispose();
          for (const m of Array.isArray(assetMesh.material) ? assetMesh.material : [assetMesh.material]) m.dispose();
        }
      });
      if (disposed) { geometry.dispose(); return; }
      const base = new Float32Array(geometry.getAttribute('position').array);
      const targets = (geometry.morphAttributes.position ?? []) as BufferAttribute[];
      // Morphing is applied once per tracking update; the GPU receives only the resulting geometry.
      geometry.morphAttributes = {};
      mesh = new Mesh(geometry, material);
      mesh.frustumCulled = false;
      scene.add(mesh);
      let previous: number[] = [];
      update = () => {
        if (!mesh) return;
        const weights = names.map((name) => trackingWeight(values, name));
        if (weights.some((weight, index) => weight !== previous[index])) {
          deformFace(geometry, base, targets, weights);
          previous = weights;
        }
        mesh.quaternion.copy(headRotation(values));
        render();
      };
      update();
    }, undefined, (e) => { if (!disposed) error = `Could not load the face: ${String(e)}`; });
    return () => {
      disposed = true; update = undefined; resize.disconnect(); cancelAnimationFrame(animation);
      renderer.domElement.removeEventListener('webglcontextlost', contextLost);
      mesh?.geometry.dispose(); material.dispose(); renderer.dispose();
    };
  });
</script>

<div class="face-viewport" bind:this={host}>
  <canvas bind:this={canvas} aria-label="Live 3D face preview"></canvas>
  {#if error}<p role="alert">{error}</p>{/if}
</div>
<style>
  .face-viewport { display: grid; place-items: center; width: 100%; height: 100%; min-height: 0; }
  .face-viewport :global(canvas) { grid-area: 1 / 1; max-width: 100%; max-height: 100%; }
  p { color: var(--bad); padding: 20px; }
</style>
