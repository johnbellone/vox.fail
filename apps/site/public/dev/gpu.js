const a = await navigator.gpu.requestAdapter(); const d = await a.requestDevice();
const ctx = document.getElementById('c').getContext('webgpu');
ctx.configure({ device: d, format: navigator.gpu.getPreferredCanvasFormat(), alphaMode: 'opaque' });
function frame() { const e = d.createCommandEncoder(); const p = e.beginRenderPass({ colorAttachments: [{ view: ctx.getCurrentTexture().createView(), clearValue: { r: 0.9, g: 0.3, b: 0.2, a: 1 }, loadOp: 'clear', storeOp: 'store' }] }); p.end(); d.queue.submit([e.finish()]); requestAnimationFrame(frame); }
frame(); window.done = true;
