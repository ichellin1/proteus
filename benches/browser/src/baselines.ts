/**
 * Two renderers written by hand with WebGL2, to compare Proteus with. Both
 * draw the scene as rounded rectangles, with the same corner radius and
 * edge smoothing, and both redraw it every frame, as Proteus does.
 *
 * - **Naive:** one draw call per component, setting its rectangle and color
 *   as uniforms first. The usual way to draw a UI by hand.
 * - **Batched:** every component in one instanced draw call, with the
 *   rectangles and colors in a buffer that's uploaded again every frame.
 *   This is how Proteus draws, done by hand.
 *
 * Their shader only draws a rounded rectangle. Proteus's also handles
 * borders, shadows, glows and textures, so it does more work per pixel.
 */

import { HEIGHT, type Rect, WIDTH } from "./scene";

export interface Renderer {
  /** Draws one frame. */
  draw(): void;
  /** Frees the GPU resources. */
  dispose(): void;
}

const FRAGMENT = `#version 300 es
precision highp float;
in vec2 v_local;
in vec2 v_half;
in vec3 v_color;
uniform float u_radius;
out vec4 out_color;
void main() {
  // Signed distance to a rounded rectangle, smoothed over one pixel.
  vec2 q = abs(v_local) - v_half + u_radius;
  float d = length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - u_radius;
  float alpha = clamp(0.5 - d, 0.0, 1.0);
  out_color = vec4(v_color * alpha, alpha);
}`;

const NAIVE_VERTEX = `#version 300 es
in vec2 a_corner;
uniform vec4 u_rect;   // center x, center y, width, height, in CSS pixels
uniform vec3 u_color;
uniform vec2 u_viewport;
out vec2 v_local;
out vec2 v_half;
out vec3 v_color;
void main() {
  v_half = u_rect.zw * 0.5;
  v_local = a_corner * v_half;
  v_color = u_color;
  vec2 pixel = u_rect.xy + v_local;
  gl_Position = vec4(pixel / u_viewport * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 0.0, 1.0);
}`;

const BATCHED_VERTEX = `#version 300 es
in vec2 a_corner;
in vec4 a_rect;
in vec3 a_color;
uniform vec2 u_viewport;
out vec2 v_local;
out vec2 v_half;
out vec3 v_color;
void main() {
  v_half = a_rect.zw * 0.5;
  v_local = a_corner * v_half;
  v_color = a_color;
  vec2 pixel = a_rect.xy + v_local;
  gl_Position = vec4(pixel / u_viewport * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 0.0, 1.0);
}`;

function program(gl: WebGL2RenderingContext, vertex: string): WebGLProgram {
  const compile = (type: number, source: string) => {
    const shader = gl.createShader(type)!;
    gl.shaderSource(shader, source);
    gl.compileShader(shader);
    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
      throw new Error(gl.getShaderInfoLog(shader) ?? "shader failed to compile");
    }
    return shader;
  };
  const p = gl.createProgram()!;
  gl.attachShader(p, compile(gl.VERTEX_SHADER, vertex));
  gl.attachShader(p, compile(gl.FRAGMENT_SHADER, FRAGMENT));
  gl.linkProgram(p);
  if (!gl.getProgramParameter(p, gl.LINK_STATUS)) {
    throw new Error(gl.getProgramInfoLog(p) ?? "program failed to link");
  }
  return p;
}

/** The four corners of a unit square, drawn as a triangle strip. */
function cornerBuffer(gl: WebGL2RenderingContext): WebGLBuffer {
  const buffer = gl.createBuffer()!;
  gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);
  return buffer;
}

function begin(gl: WebGL2RenderingContext) {
  gl.viewport(0, 0, gl.drawingBufferWidth, gl.drawingBufferHeight);
  gl.clearColor(0, 0, 0, 1);
  gl.clear(gl.COLOR_BUFFER_BIT);
  gl.enable(gl.BLEND);
  gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
}

export function naive(gl: WebGL2RenderingContext, rects: Rect[], radius: number): Renderer {
  const p = program(gl, NAIVE_VERTEX);
  const corners = cornerBuffer(gl);
  const vao = gl.createVertexArray()!;
  gl.bindVertexArray(vao);
  const corner = gl.getAttribLocation(p, "a_corner");
  gl.enableVertexAttribArray(corner);
  gl.vertexAttribPointer(corner, 2, gl.FLOAT, false, 0, 0);
  const rect = gl.getUniformLocation(p, "u_rect");
  const color = gl.getUniformLocation(p, "u_color");

  return {
    draw() {
      begin(gl);
      gl.useProgram(p);
      gl.bindVertexArray(vao);
      gl.uniform2f(gl.getUniformLocation(p, "u_viewport"), WIDTH, HEIGHT);
      gl.uniform1f(gl.getUniformLocation(p, "u_radius"), radius);
      for (const r of rects) {
        gl.uniform4f(rect, r.x, r.y, r.width, r.height);
        gl.uniform3f(color, r.r, r.g, r.b);
        gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
      }
    },
    dispose() {
      gl.deleteProgram(p);
      gl.deleteBuffer(corners);
      gl.deleteVertexArray(vao);
    },
  };
}

export function batched(gl: WebGL2RenderingContext, rects: Rect[], radius: number): Renderer {
  const p = program(gl, BATCHED_VERTEX);
  const corners = cornerBuffer(gl);
  const vao = gl.createVertexArray()!;
  gl.bindVertexArray(vao);
  const corner = gl.getAttribLocation(p, "a_corner");
  gl.enableVertexAttribArray(corner);
  gl.vertexAttribPointer(corner, 2, gl.FLOAT, false, 0, 0);

  // Seven floats per component: the rectangle, then the color.
  const data = new Float32Array(rects.length * 7);
  const instances = gl.createBuffer()!;
  gl.bindBuffer(gl.ARRAY_BUFFER, instances);
  gl.bufferData(gl.ARRAY_BUFFER, data.byteLength, gl.DYNAMIC_DRAW);
  const rectAttribute = gl.getAttribLocation(p, "a_rect");
  const colorAttribute = gl.getAttribLocation(p, "a_color");
  gl.enableVertexAttribArray(rectAttribute);
  gl.vertexAttribPointer(rectAttribute, 4, gl.FLOAT, false, 28, 0);
  gl.vertexAttribDivisor(rectAttribute, 1);
  gl.enableVertexAttribArray(colorAttribute);
  gl.vertexAttribPointer(colorAttribute, 3, gl.FLOAT, false, 28, 16);
  gl.vertexAttribDivisor(colorAttribute, 1);

  return {
    draw() {
      // Filled and uploaded every frame, as Proteus uploads its instances.
      rects.forEach((r, i) => data.set([r.x, r.y, r.width, r.height, r.r, r.g, r.b], i * 7));
      begin(gl);
      gl.useProgram(p);
      gl.bindVertexArray(vao);
      gl.bindBuffer(gl.ARRAY_BUFFER, instances);
      gl.bufferSubData(gl.ARRAY_BUFFER, 0, data);
      gl.uniform2f(gl.getUniformLocation(p, "u_viewport"), WIDTH, HEIGHT);
      gl.uniform1f(gl.getUniformLocation(p, "u_radius"), radius);
      gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, rects.length);
    },
    dispose() {
      gl.deleteProgram(p);
      gl.deleteBuffer(corners);
      gl.deleteBuffer(instances);
      gl.deleteVertexArray(vao);
    },
  };
}
