// The play page's check before loading the client: no WebGPU, or no GPU behind it, is said
// plainly (Firefox gets the setting to switch on), rather than the client stopping at "Starting…";
// and which detail profile the device gets.

import { expect, test } from "bun:test";
import { deviceDetail, webgpuProblem } from "./wasm";

const FIREFOX = "Mozilla/5.0 (X11; Linux x86_64; rv:146.0) Gecko/20100101 Firefox/146.0";
const CHROME = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0 Safari/537.36";

test("no WebGPU: Firefox is told which setting to switch on", async () => {
  const why = await webgpuProblem(FIREFOX, undefined);
  expect(why).toContain("dom.webgpu.enabled");
  expect(await webgpuProblem(CHROME, undefined)).toContain("doesn't offer");
});

test("WebGPU without a graphics card says so", async () => {
  expect(await webgpuProblem(CHROME, { requestAdapter: async () => null })).toContain("couldn't get a graphics card");
  expect(
    await webgpuProblem(CHROME, {
      requestAdapter: async () => {
        throw new Error("blocked");
      },
    }),
  ).toContain("couldn't get a graphics card");
});

test("a GPU to draw with: no problem", async () => {
  expect(await webgpuProblem(FIREFOX, { requestAdapter: async () => ({}) })).toBeUndefined();
});

test("phones and tablets get the mobile profile, computers the browser one", () => {
  expect(deviceDetail({ coarse: true })).toBe("mobile");
  expect(deviceDetail({ coarse: false, mobile: true })).toBe("mobile");
  expect(deviceDetail({ coarse: false, mobile: false })).toBe("browser");
  expect(deviceDetail({ coarse: false })).toBe("browser");
});
