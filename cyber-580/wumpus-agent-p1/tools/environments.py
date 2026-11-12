import random
import numbers
import json

# ______________________________________________________________________________


class Thing:
    """This represents any physical object that can appear in an Environment.
    You subclass Thing to get the things you want. Each thing can have a
    .__name__  slot (used for output only)."""

    def __repr__(self):
        return '<{}>'.format(getattr(self, '__name__', self.__class__.__name__))


class Agent(Thing):
    pass
# ______________________________________________________________________________


class Environment:
    """Abstract class representing an Environment. 'Real' Environment classes
    inherit from this. Your Environment will typically need to implement:
        percept:           Define the percept that an agent sees.
        execute_action:    Define the effects of executing an action.
                           Also update the agent.performance slot.
    The environment keeps a list of .things and .agents (which is a subset
    of .things). Each agent has a .performance slot, initialized to 0.
    Each thing has a .location slot, even though some environments may not
    need this."""

    def __init__(self):
        self.things = []
        self.agents = []



    def list_things_at(self, location, tclass=Thing):
        """Return all things exactly at a given location."""
        if isinstance(location, numbers.Number):
            return [thing for thing in self.things
                    if thing.location == location and isinstance(thing, tclass)]
        return [thing for thing in self.things
                if all(x == y for x, y in zip(thing.location, location)) and isinstance(thing, tclass)]

    def some_things_at(self, location, tclass=Thing):
        """Return true if at least one of the things at location
        is an instance of class tclass (or a subclass)."""
        return self.list_things_at(location, tclass) != []

    def add_thing(self, thing, location=None):
        """Add a thing to the environment, setting its location. For
        convenience, if thing is an agent program we make a new agent
        for it. (Shouldn't need to override this.)"""
        if not isinstance(thing, Thing):
            thing = Agent(thing)
        if thing in self.things:
            print("Can't add the same thing twice")
        else:
            thing.location = location if location is not None else self.default_location(thing)
            self.things.append(thing)
            if isinstance(thing, Agent):
                thing.performance = 0
                self.agents.append(thing)

    def delete_thing(self, thing):
        """Remove a thing from the environment."""
        try:
            self.things.remove(thing)
        except ValueError as e:
            print(e)
            print("  in Environment delete_thing")
            print("  Thing to be removed: {} at {}".format(thing, thing.location))
            print("  from list: {}".format([(thing, thing.location) for thing in self.things]))
        if thing in self.agents:
            self.agents.remove(thing)


class XYEnvironment(Environment):
    """This class is for environments on a 2D plane, with locations
    labelled by (x, y) points, either discrete or continuous.

    Agents perceive things within a radius. Each agent in the
    environment has a .location slot which should be a location such
    as (0, 1), and a .holding slot, which should be a list of things
    that are held."""

    def __init__(self, width=10, height=10):
        super().__init__()

        self.width = width
        self.height = height
        self.observers = []
        # Sets iteration start and end (no walls).
        self.x_start, self.y_start = (0, 0)
        self.x_end, self.y_end = (self.width, self.height)

    perceptible_distance = 1

    def add_thing(self, thing, location=None, exclude_duplicate_class_items=False):
        """Add things to the world. If (exclude_duplicate_class_items) then the item won't be
        added if the location has at least one item of the same class."""
        if location is None:
            super().add_thing(thing)
        elif self.is_inbounds(location):
            if (exclude_duplicate_class_items and
                    any(isinstance(t, thing.__class__) for t in self.list_things_at(location))):
                return
            super().add_thing(thing, location)

    def is_inbounds(self, location):
        """Checks to make sure that the location is inbounds (within walls if we have walls)"""
        x, y = location
        return not (x < self.x_start or x > self.x_end or y < self.y_start or y > self.y_end)

    def random_location_inbounds(self, exclude=None):
        """Returns a random location that is inbounds (within walls if we have walls)"""
        location = (random.randint(self.x_start, self.x_end-1),
                    random.randint(self.y_start, self.y_end-1))
        if exclude is not None:
            while location == exclude:
                location = (random.randint(self.x_start, self.x_end),
                            random.randint(self.y_start, self.y_end))
        return location

    def delete_thing(self, thing):
        """Deletes thing, and everything it is holding (if thing is an agent)"""
        if isinstance(thing, Agent):
            del thing.holding

        super().delete_thing(thing)
        for obs in self.observers:
            obs.thing_deleted(thing)

    def add_walls(self):
        """Put walls around the entire perimeter of the grid."""
        for x in range(self.width):
            self.add_thing(Wall(), (x, 0))
            self.add_thing(Wall(), (x, self.height - 1))
        for y in range(1, self.height - 1):
            self.add_thing(Wall(), (0, y))
            self.add_thing(Wall(), (self.width - 1, y))

        # Updates iteration start and end (with walls).
        self.x_start, self.y_start = (1, 1)
        self.x_end, self.y_end = (self.width - 1, self.height - 1)

class Obstacle(Thing):
    """Something that can cause a bump, preventing an agent from
    moving into the same square it's in."""
    pass


class Wall(Obstacle):
    pass




# ______________________________________________________________________________
# The Wumpus World


class Gold(Thing):

    def __eq__(self, rhs):
        """All Gold are equal"""
        return rhs.__class__ == Gold

    pass


class Bump(Thing):
    pass


class Glitter(Thing):
    pass


class Pit(Thing):
    pass


class Breeze(Thing):
    pass


class Arrow(Thing):
    pass


class Scream(Thing):
    pass


class Wumpus(Agent):
    pass


class Stench(Thing):
    pass


class Explorer(Agent):
    pass


class WumpusEnvironment(XYEnvironment):
    # Room should be 4x4 grid of rooms. The extra 2 for walls
    performance = None
    def __init__(self, import_filename=None, width=6, height=6, pit_probability=0.2):
        self.starting_location=(1,1) #Chris said to fix this in place.
        self.pit_probability = pit_probability
        super().__init__(width, height)
        if import_filename is not None:
            self._import_json(import_filename)
        else:
            self.init_world()

    def init_world(self):
        """Spawn items in the world based on probabilities from the book"""

        "WALLS"
        self.add_walls()

        "PITS"
        for x in range(self.x_start, self.x_end):
            for y in range(self.y_start, self.y_end):
                if (x, y) == self.starting_location:
                    continue
                if random.random() < self.pit_probability:
                    self.add_thing(Pit(), (x, y), True)
                    self.add_thing(Breeze(), (x - 1, y), True)
                    self.add_thing(Breeze(), (x, y - 1), True)
                    self.add_thing(Breeze(), (x + 1, y), True)
                    self.add_thing(Breeze(), (x, y + 1), True)

        "WUMPUS"
        w_x, w_y = self.random_location_inbounds(exclude=self.starting_location)
        self.add_thing(Wumpus(), (w_x, w_y), True)
        self.add_thing(Stench(), (w_x - 1, w_y), True)
        self.add_thing(Stench(), (w_x + 1, w_y), True)
        self.add_thing(Stench(), (w_x, w_y - 1), True)
        self.add_thing(Stench(), (w_x, w_y + 1), True)

        "GOLD"
        self.add_thing(Gold(), self.random_location_inbounds(exclude=self.starting_location), True)

        "AGENT"
        self.add_thing(Explorer(), self.starting_location, True)

    def _private_get_world(self, show_walls=True):
        """Return the items in the world"""
        result = []
        x_start, y_start = (0, 0) if show_walls else (1, 1)

        if show_walls:
            x_end, y_end = self.width, self.height
        else:
            x_end, y_end = self.width - 1, self.height - 1

        for x in range(x_start, x_end):
            row = []
            for y in range(y_start, y_end):
                row.append(self.list_things_at((x, y)))
            result.append(row)
        return result
    def get_world(self):
        result = self._private_get_world()
        for x in range(len(result)):
            for y in range(len(result[x])):
                if len(result[x][y]):
                    result[x][y] = [z.__class__.__name__ for z in result[x][y]]
        return result
    
    def export_json(self, filename):
        world_dims = (self.width, self.height)
        world = self._private_get_world()

        excluded_items = ["Breeze", "Stench", "Glitter", "Bump", "Scream", "Arrow"]
        to_export = {'world_dims': world_dims, 'world': []}
        for x in range(len(world)):
            row = []
            for y in range(len(world[x])):
                cell = []
                for item in world[x][y]:
                    if item.__class__.__name__ not in excluded_items:
                        cell.append(item.__class__.__name__)
                row.append(cell)
            to_export['world'].append(row)
        json.dump(to_export, open(filename, "w"), indent=4)
    
    def _import_json(self, filename):
        imported = json.load(open(filename, "r"))
        (self.width, self.height) = tuple(imported['world_dims'])
        world = imported['world']
        for x in range(len(world)):
            for y in range(len(world[x])):
                for item in world[x][y]:
                    if item is None:
                        continue
                    if item == "Explorer":
                        self.add_thing(Explorer(), self.starting_location, True) #fixed to 1,1 as per Chris's instruction
                        continue
                    if item == "Wumpus":
                        self.add_thing(Wumpus(), (x, y), True)
                        self.add_thing(Stench(), (x - 1, y), True)
                        self.add_thing(Stench(), (x, y - 1), True)
                        self.add_thing(Stench(), (x + 1, y), True)
                        self.add_thing(Stench(), (x, y + 1), True)
                        continue
                    if item == "Pit":
                        self.add_thing(Pit(), (x, y), True)
                        self.add_thing(Breeze(), (x - 1, y), True)
                        self.add_thing(Breeze(), (x, y - 1), True)
                        self.add_thing(Breeze(), (x + 1, y), True)
                        self.add_thing(Breeze(), (x, y + 1), True)
                        continue
                    if item == "Gold":
                        self.add_thing(Gold(), (x, y), True)
                        continue
                    
                    cls = globals()[item]
                    self.add_thing(cls(), (x, y), True)


# ______________________________________________________________________________
