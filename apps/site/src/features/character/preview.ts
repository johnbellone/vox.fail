// The character maker's turntable: your character, idling, with what you wear, standing on a
// patch of the Void's floor. Hats and face pieces sit on the head and held things in the right
// hand, placed as the game places them (data/head_fit.json, data/bare_heads.json).

import * as THREE from "three";
import { GLTFLoader, type GLTF } from "three/examples/jsm/loaders/GLTFLoader.js";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import type { Slot } from "../../api/types";

export interface HeadFit {
  top: number;
  width: number;
  front: number;
  eye_y: number;
  mouth_y: number;
}

export interface Worn {
  slot: Slot;
  id: string;
  file: string;
}

const DEFAULT_FIT: HeadFit = { top: 0.62, width: 0.7, front: 0.31, eye_y: 0.25, mouth_y: 0.07 };

export class Preview {
  private readonly cache = new Map<string, Promise<GLTF>>();
  private readonly loader = new GLTFLoader();
  private readonly renderer: THREE.WebGLRenderer;
  private readonly scene = new THREE.Scene();
  private readonly camera = new THREE.PerspectiveCamera(30, 1, 0.1, 50);
  private readonly controls: OrbitControls;
  private readonly timer = new THREE.Timer();
  private readonly resizer: ResizeObserver;
  private mixer: THREE.AnimationMixer | null = null;
  private current: THREE.Object3D | null = null;
  private framed = false;
  private want = 0;

  constructor(
    private readonly canvas: HTMLCanvasElement,
    private readonly fits: Record<string, HeadFit> = {},
    private readonly bare: Record<string, number> = {},
  ) {
    const r = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true });
    r.setPixelRatio(Math.min(devicePixelRatio, 2));
    r.outputColorSpace = THREE.SRGBColorSpace;
    r.toneMapping = THREE.ACESFilmicToneMapping;
    r.shadowMap.enabled = true;
    this.renderer = r;
    this.camera.position.set(1.6, 1.45, 4.2);
    const c = new OrbitControls(this.camera, canvas);
    c.target.set(0, 0.95, 0);
    c.enablePan = false;
    c.maxPolarAngle = Math.PI * 0.55;
    c.autoRotate = !matchMedia("(prefers-reduced-motion: reduce)").matches;
    c.autoRotateSpeed = 1.2;
    this.controls = c;
    this.scene.add(new THREE.HemisphereLight(0xf4f7ff, 0xc9cfdc, 2.2));
    const sun = new THREE.DirectionalLight(0xffffff, 2.4);
    sun.position.set(2.5, 5, 3);
    sun.castShadow = true;
    sun.shadow.mapSize.set(1024, 1024);
    sun.shadow.camera.left = sun.shadow.camera.bottom = -2;
    sun.shadow.camera.right = sun.shadow.camera.top = 2;
    this.scene.add(sun);
    const floor = new THREE.Mesh(new THREE.CircleGeometry(1.6, 64), new THREE.MeshStandardMaterial({ color: 0xeef1f6, roughness: 0.25 }));
    floor.rotation.x = -Math.PI / 2;
    floor.receiveShadow = true;
    this.scene.add(floor);
    this.resizer = new ResizeObserver(() => this.resize());
    this.resizer.observe(canvas);
    this.resize();
    r.setAnimationLoop((t) => this.frame(t));
  }

  /** Stop drawing and let go of the GPU. */
  dispose() {
    this.renderer.setAnimationLoop(null);
    this.resizer.disconnect();
    this.controls.dispose();
    this.renderer.dispose();
  }

  private resize() {
    const w = this.canvas.clientWidth || 300;
    const h = this.canvas.clientHeight || 375;
    this.renderer.setSize(w, h, false);
    this.camera.aspect = w / h;
    this.camera.updateProjectionMatrix();
  }

  private load(file: string): Promise<GLTF> {
    let p = this.cache.get(file);
    if (!p) {
      p = this.loader.loadAsync(`/assets/${file}`);
      this.cache.set(file, p);
    }
    return p;
  }

  /** Show `model` wearing `worn`. */
  async show(model: string | null, worn: Worn[]) {
    if (!model) return;
    const ticket = ++this.want;
    const gltf = await this.load(model);
    const items = await Promise.all(worn.map((w) => this.load(w.file).then((g) => ({ ...w, g }))));
    if (ticket !== this.want) return; // something newer was asked for meanwhile
    if (this.current) this.scene.remove(this.current);
    // a fresh copy each time, so what it wore before is gone
    const body = gltf.scene.clone(true);
    castShadows(body);
    this.mixer = new THREE.AnimationMixer(body);
    const idle = gltf.animations.find((a) => a.name === "Idle") ?? gltf.animations[0];
    if (idle) this.mixer.clipAction(idle).play();
    const file = model.split("/").pop() ?? model;
    const fit = this.fits[file] ?? DEFAULT_FIT;
    const headScale = Math.min(1.35, Math.max(0.85, fit.width / 0.62));
    const head = body.getObjectByName("Head");
    const hand = body.getObjectByName("LowerArm_R");
    const hat = items.some((it) => it.slot === "hat");
    // a character with its own cap takes it off for a hat: its bare head is under it
    const bare = body.getObjectByName("Head_bare");
    const bareTop = this.bare[file];
    if (bare) bare.visible = hat && bareTop !== undefined;
    if (bare?.visible && head instanceof THREE.Mesh) {
      const m = (head.material as THREE.Material).clone();
      m.visible = false;
      head.material = m;
    }
    for (const it of items) {
      const thing = it.g.scene.clone(true);
      castShadows(thing);
      if (it.slot === "hat" && head) {
        const top = bare?.visible && bareTop !== undefined ? bareTop : fit.top;
        thing.position.set(0, top - 0.01, 0);
        thing.scale.setScalar(headScale);
        head.add(thing);
      } else if (it.slot === "face" && head) {
        const y = it.id === "moustache" ? fit.mouth_y + 0.06 : it.id === "clown" ? (fit.eye_y + fit.mouth_y) / 2 : fit.eye_y;
        thing.position.set(0, y, fit.front + 0.005);
        thing.scale.setScalar(headScale);
        head.add(thing);
      } else if (it.slot === "hand" && hand) {
        thing.position.set(0, -0.25, 0.05);
        hand.add(thing);
      }
    }
    this.current = body;
    this.scene.add(body);
    if (!this.framed) {
      // frame the first character: all of them, a little above centre
      this.framed = true;
      const box = new THREE.Box3().setFromObject(body);
      const h = box.max.y - box.min.y;
      this.controls.target.set(0, box.min.y + h * 0.52, 0);
      const d = (h * 0.62) / Math.tan((this.camera.fov * Math.PI) / 360);
      this.camera.position.set(d * 0.38, box.min.y + h * 0.62, d * 0.92);
      this.controls.minDistance = d * 0.5;
      this.controls.maxDistance = d * 1.8;
      this.controls.update();
    }
  }

  private frame(time: number) {
    this.timer.update(time);
    const dt = Math.min(this.timer.getDelta(), 0.1);
    this.mixer?.update(dt);
    this.controls.update();
    this.renderer.render(this.scene, this.camera);
  }
}

function castShadows(o: THREE.Object3D) {
  o.traverse((x) => {
    if (x instanceof THREE.Mesh) x.castShadow = true;
  });
}
