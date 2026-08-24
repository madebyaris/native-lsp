/** Mixed-language fixture: TypeScript API client. */
export type UserId = string;

export interface User {
  id: UserId;
  email: string;
}

export enum Role {
  Admin = "admin",
  Editor = "editor",
}

export class ApiClient {
  constructor(private readonly base: string) {}

  async fetchUser(id: UserId): Promise<User> {
    const res = await fetch(`${this.base}/users/${id}`);
    return res.json();
  }
}

export function isAdmin(role: Role): boolean {
  return role === Role.Admin;
}
