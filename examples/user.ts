export type Role = "admin" | "user";

export interface Profile {
  role: Role;
  active: boolean;
}

export class User {
  id: string;
  name: string;
  age?: number;
  tags: string[];
  profile: Profile;
  address: {
    street: string;
    city: string;
    zip?: number;
  };
}
