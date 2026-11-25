// The character creator's turntable: your character, idling, with what you wear, standing on
// a patch of the Void's floor. Hats and face pieces sit on the head and held things in the
// right hand, placed as the game places them (data/head_fit.json, data/bare_heads.json).

import * as THREE from "/vendor/three/three.module.min.js";
import { GLTFLoader } from "/vendor/three/GLTFLoader.js";
import { OrbitControls } from "/vendor/three/OrbitControls.js";

const DEFAULT_FIT = { top: 0.62, width: 0.7, front: 0.31, eye_y: 0.25, mouth_y: 0.07 };

export class Preview {
  constructor(canvas, fits, bare) {
    this.fits = fits || {};
    this.bare = bare || {};
    this.cache = new Map();
    this.loader = new GLTFLoader();
    this.want = 0;
    const r = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true });
    r.setPixelRatio(Math.min(devicePixelRatio, 2));
    r.outputColorSpace = THREE.SRGBColorSpace;
    r.toneMapping = THREE.ACESFilmicToneMapping;
    r.shadowMap.enabled = true;
    this.renderer = r;
    this.scene = new THREE.Scene();
    this.camera = new THREE.PerspectiveCamera(30, 1, 0.1, 50);
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
    this.clock = new THREE.Clock();
    this.mixer = null;
    this.current = null;
    this.framed = false;
    new ResizeObserver(() => this.resize()).observe(canvas);
    this.resize();
    r.setAnimationLoop(() => this.frame());
  }

  resize() {
    const c = this.renderer.domElement;
    const w = c.clientWidth || 300, h = c.clientHeight || 375;
    this.renderer.setSize(w, h, false);
    this.camera.aspect = w / h;
    this.camera.updateProjectionMatrix();
  }

  load(file) {
    if (!this.cache.has(file)) this.cache.set(file, this.loader.loadAsync(`/assets/${file}`));
    return this.cache.get(file);
  }

  /** Show `model` wearing `worn` ([{slot, id, file}]). */
  async show(model, worn) {
    if (!model) return;
    const ticket = ++this.want;
    const gltf = await this.load(model);
    const items = await Promise.all(worn.map((w) => this.load(w.file).then((g) => ({ ...w, g }))));
    if (ticket !== this.want) return; // something newer was asked for meanwhile
    if (this.current) this.scene.remove(this.current);
    // a fresh copy each time, so what it wore before is gone
    const body = gltf.scene.clone(true);
    body.traverse((o) => { if (o.isMesh) o.castShadow = true; });
    this.mixer = new THREE.AnimationMixer(body);
    const idle = gltf.animations.find((a) => a.name === "Idle") || gltf.animations[0];
    if (idle) this.mixer.clipAction(idle).play();
    const file = model.split("/").pop();
    const fit = this.fits[file] || DEFAULT_FIT;
    const headScale = Math.min(1.35, Math.max(0.85, fit.width / 0.62));
    const head = body.getObjectByName("Head");
    const hand = body.getObjectByName("LowerArm_R");
    const hat = items.some((it) => it.slot === "hat");
    // a character with its own cap takes it off for a hat: its bare head is under it
    const bare = body.getObjectByName("Head_bare");
    const bareTop = this.bare[file];
    if (bare) bare.visible = hat && bareTop !== undefined;
    if (bare && bare.visible && head && head.isMesh) {
      head.material = head.material.clone();
      head.material.visible = false;
    }
    for (const it of items) {
      const thing = it.g.scene.clone(true);
      thing.traverse((o) => { if (o.isMesh) o.castShadow = true; });
      if (it.slot === "hat" && head) {
        const top = bare && bare.visible ? bareTop : fit.top;
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

  frame() {
    const dt = Math.min(this.clock.getDelta(), 0.1);
    if (this.mixer) this.mixer.update(dt);
    this.controls.update();
    this.renderer.render(this.scene, this.camera);
  }
}
