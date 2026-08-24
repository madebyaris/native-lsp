# Mixed-language fixture: checkout service.
class Store:
    def __init__(self, catalog):
        self.catalog = catalog

    def checkout(self, cart_id: str) -> dict:
        return {"cart": cart_id, "total": self.total(cart_id)}

    def total(self, cart_id: str) -> int:
        return len(cart_id)


def format_price(cents: int) -> str:
    return f"${cents / 100:.2f}"
