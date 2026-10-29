export default {
  async fetch(req, env) {
    const body = await req.json();
    const t = Date.now();
    try {
      const out = await env.AI.run(body.model || "@cf/cloudflare/clef-flash", body.input);
      return Response.json({ ms: Date.now() - t, out });
    } catch (e) { return Response.json({ ms: Date.now() - t, error: String(e) }, { status: 500 }); }
  }
};
