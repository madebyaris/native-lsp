-- Mixed-language fixture: storefront schema.
CREATE TABLE posts (
  id BIGINT PRIMARY KEY,
  slug VARCHAR(191) NOT NULL,
  content TEXT
);

CREATE UNIQUE INDEX posts_slug ON posts (slug);

CREATE VIEW published_posts AS
SELECT id, slug FROM posts;
