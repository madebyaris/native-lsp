/** Checkout API client. */
export type CartId = string;

export interface LineItem {
  id: string;
  cents: number;
}

export enum CheckoutState {
  Idle = "idle",
  Paid = "paid",
}

export class CheckoutClient {
  constructor(private readonly base: string) {}

  async submit(id: CartId): Promise<CheckoutState> {
    const res = await fetch(`${this.base}/checkout/${id}`);
    const body = await res.json();
    return body.state === "paid" ? CheckoutState.Paid : CheckoutState.Idle;
  }
}

export function formatMoney(cents: number): string {
  return `$${(cents / 100).toFixed(2)}`;
}
