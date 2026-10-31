const body = new TextEncoder().encode('hello');

export default {
  fetch: () =>
    new Response(
      new ReadableStream({
        start(controller) {
          controller.enqueue(body);
          controller.close();
        },
      }),
    ),
};
