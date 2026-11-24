// Everything a non-developer may want to change on the landing.
window.JARVIS = {
  // Pro prices shown on the page, in USD.
  price: { monthly: 7, yearly: 59 },
  // Paddle Billing (paddle.com). Until `token` and the price IDs are set the
  // Pro button opens the waitlist instead of a checkout.
  // token: a client-side token (Developer tools → Authentication), "live_…" or "test_…".
  paddle: {
    token: "",
    prices: { monthly: "", yearly: "" },
  },
};
