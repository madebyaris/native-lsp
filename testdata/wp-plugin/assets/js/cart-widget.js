/** Storefront cart widget. */
export class CartWidget {
  constructor(root) {
    this.root = root;
  }

  render(items) {
    return items.map((item) => formatPrice(item.price)).join("\n");
  }
}

export function formatPrice(cents) {
  return `$${(cents / 100).toFixed(2)}`;
}

export const checkout = async (cartId) => {
  return fetch(`/wp-json/native-shop/v1/checkout/${cartId}`);
};
