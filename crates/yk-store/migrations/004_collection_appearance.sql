-- Colour and icon for collections and smart collections.
--
-- The colour stores a *palette name*, not a hex value: the palette is defined
-- by the theme, so a named colour keeps its meaning when the theme changes and
-- an unreadable combination is impossible to create. Same reasoning for icons —
-- a name resolves to a drawing the app already ships.
ALTER TABLE collections ADD COLUMN color TEXT;
ALTER TABLE collections ADD COLUMN icon TEXT;

ALTER TABLE smart_collections ADD COLUMN color TEXT;
ALTER TABLE smart_collections ADD COLUMN icon TEXT;
