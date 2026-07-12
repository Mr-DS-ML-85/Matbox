/* ============================================================
   MATBOX — 3D Scene
   Wireframe surface plot (MATLAB-style peaks) + matrix grid bg.
   Pure Three.js, no build step.
   ============================================================ */
(function () {
  if (typeof THREE === "undefined") {
    console.warn("[MATBOX] Three.js not loaded, skipping 3D scene.");
    return;
  }

  const canvas = document.getElementById("three-canvas");
  if (!canvas) return;

  // -------- Renderer
  const renderer = new THREE.WebGLRenderer({
    canvas,
    antialias: true,
    alpha: true,
    powerPreference: "high-performance",
  });
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
  renderer.setClearColor(0x000000, 0);

  // -------- Scene + camera
  const scene = new THREE.Scene();
  scene.fog = new THREE.FogExp2(0x07090c, 0.06);

  const camera = new THREE.PerspectiveCamera(45, 1, 0.1, 100);
  camera.position.set(4.5, 3.2, 5.5);
  camera.lookAt(0, 0, 0);

  // -------- Surface: MATLAB "peaks" function
  // z = 3(1-x)^2 * exp(-x^2 - (y+1)^2) - 10(x/5 - x^3 - y^5) * exp(-x^2 - y^2) - 1/3 * exp(-(x+1)^2 - y^2)
  function peaks(x, y) {
    const a = 3 * ((1 - x) ** 2) * Math.exp(-(x * x) - ((y + 1) ** 2));
    const b = 10 * (x / 5 - (x ** 3) - (y ** 5)) * Math.exp(-(x * x) - y * y);
    const c = (1 / 3) * Math.exp(-((x + 1) ** 2) - y * y);
    return a - b - c;
  }

  const SIZE = 90;          // grid resolution
  const SCALE = 1.6;        // x/y domain
  const Z_SCALE = 0.55;     // vertical exaggeration

  const geometry = new THREE.PlaneGeometry(
    SCALE * 2,
    SCALE * 2,
    SIZE - 1,
    SIZE - 1
  );
  geometry.rotateX(-Math.PI / 2);

  const pos = geometry.attributes.position;
  const colors = new Float32Array(pos.count * 3);
  for (let i = 0; i < pos.count; i++) {
    const x = pos.getX(i);
    const z = pos.getZ(i);
    const y = peaks(x, z) * Z_SCALE;
    pos.setY(i, y);

    // color gradient by height
    const t = THREE.MathUtils.clamp((y + 1.4) / 3.0, 0, 1);
    const col = new THREE.Color();
    col.setHSL(0.45 - t * 0.45, 0.85, 0.35 + t * 0.25);
    colors[i * 3 + 0] = col.r;
    colors[i * 3 + 1] = col.g;
    colors[i * 3 + 2] = col.b;
  }
  geometry.setAttribute("color", new THREE.BufferAttribute(colors, 3));
  geometry.computeVertexNormals();

  // Wireframe overlay (the star)
  const wireMat = new THREE.LineBasicMaterial({
    color: 0x5af78e,
    transparent: true,
    opacity: 0.55,
  });
  const wire = new THREE.LineSegments(
    new THREE.WireframeGeometry(geometry),
    wireMat
  );
  wire.position.y = 0.001;
  scene.add(wire);

  // Translucent fill underneath
  const fillMat = new THREE.MeshBasicMaterial({
    vertexColors: true,
    transparent: true,
    opacity: 0.18,
    side: THREE.DoubleSide,
  });
  const fill = new THREE.Mesh(geometry, fillMat);
  scene.add(fill);

  // Glow accent rings
  const ringGeo = new THREE.RingGeometry(2.4, 2.42, 96);
  ringGeo.rotateX(-Math.PI / 2);
  const ringMat = new THREE.MeshBasicMaterial({
    color: 0x5af78e,
    transparent: true,
    opacity: 0.35,
  });
  const ring = new THREE.Mesh(ringGeo, ringMat);
  ring.position.y = -1.0;
  scene.add(ring);

  // -------- Background: matrix grid particles
  const particles = (function () {
    const COUNT = 600;
    const positions = new Float32Array(COUNT * 3);
    const speeds = new Float32Array(COUNT);
    for (let i = 0; i < COUNT; i++) {
      positions[i * 3 + 0] = (Math.random() - 0.5) * 24;
      positions[i * 3 + 1] = (Math.random() - 0.5) * 14;
      positions[i * 3 + 2] = (Math.random() - 0.5) * 18 - 4;
      speeds[i] = 0.0005 + Math.random() * 0.0025;
    }
    const g = new THREE.BufferGeometry();
    g.setAttribute("position", new THREE.BufferAttribute(positions, 3));
    const m = new THREE.PointsMaterial({
      color: 0x5af78e,
      size: 0.025,
      transparent: true,
      opacity: 0.55,
      sizeAttenuation: true,
    });
    return new THREE.Points(g, m);
  })();
  scene.add(particles);

  // -------- Matrix code rain (faint, far back)
  const codeRain = (function () {
    const COUNT = 80;
    const positions = new Float32Array(COUNT * 3);
    const phase = new Float32Array(COUNT);
    for (let i = 0; i < COUNT; i++) {
      positions[i * 3 + 0] = (Math.random() - 0.5) * 30;
      positions[i * 3 + 1] = Math.random() * 12 - 6;
      positions[i * 3 + 2] = -10 - Math.random() * 4;
      phase[i] = Math.random() * 100;
    }
    const g = new THREE.BufferGeometry();
    g.setAttribute("position", new THREE.BufferAttribute(positions, 3));
    const m = new THREE.PointsMaterial({
      color: 0x79e2ff,
      size: 0.05,
      transparent: true,
      opacity: 0.35,
    });
    const pts = new THREE.Points(g, m);
    pts.userData.phase = phase;
    return pts;
  })();
  scene.add(codeRain);

  // -------- Mouse interaction
  const mouse = { x: 0, y: 0, target: { x: 0, y: 0 } };
  window.addEventListener("mousemove", (e) => {
    mouse.target.x = (e.clientX / window.innerWidth) * 2 - 1;
    mouse.target.y = (e.clientY / window.innerHeight) * 2 - 1;
  });

  // -------- Resize
  function resize() {
    const rect = canvas.getBoundingClientRect();
    const w = rect.width || canvas.clientWidth;
    const h = rect.height || canvas.clientHeight;
    renderer.setSize(w, h, false);
    camera.aspect = w / h;
    camera.updateProjectionMatrix();
  }
  resize();
  window.addEventListener("resize", resize);
  // Defer-resize on first frame in case fonts shifted layout
  requestAnimationFrame(resize);

  // -------- Animate
  const clock = new THREE.Clock();
  let raf = 0;

  function animate() {
    raf = requestAnimationFrame(animate);
    const t = clock.getElapsedTime();

    // ease mouse
    mouse.x += (mouse.target.x - mouse.x) * 0.05;
    mouse.y += (mouse.target.y - mouse.y) * 0.05;

    // rotate surface slowly
    wire.rotation.y = t * 0.18 + mouse.x * 0.4;
    fill.rotation.y = wire.rotation.y;
    ring.rotation.y = -t * 0.25;

    // gentle vertical bob
    wire.position.y = Math.sin(t * 0.6) * 0.05;
    fill.position.y = wire.position.y;
    ring.position.y = -1.0 + Math.sin(t * 0.6) * 0.05;

    // particle drift
    const pp = particles.geometry.attributes.position;
    for (let i = 0; i < pp.count; i++) {
      let y = pp.getY(i);
      y -= 0.0035;
      if (y < -7) y = 7;
      pp.setY(i, y);
    }
    pp.needsUpdate = true;

    // code rain drift
    const cp = codeRain.geometry.attributes.position;
    for (let i = 0; i < cp.count; i++) {
      let y = cp.getY(i);
      y -= 0.025;
      if (y < -6) y = 6;
      cp.setY(i, y);
    }
    cp.needsUpdate = true;

    // camera follow mouse subtly
    camera.position.x = 4.5 + mouse.x * 0.8;
    camera.position.y = 3.2 + mouse.y * 0.5;
    camera.lookAt(0, 0, 0);

    renderer.render(scene, camera);
  }
  animate();

  // Pause when off-screen (battery friendly)
  document.addEventListener("visibilitychange", () => {
    if (document.hidden) cancelAnimationFrame(raf);
    else animate();
  });
})();
