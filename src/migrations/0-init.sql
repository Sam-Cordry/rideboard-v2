CREATE TABLE event (
    id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    location VARCHAR(255) NOT NULL,
    start_time TIMESTAMP WITH TIME ZONE NOT NULL,
    end_time TIMESTAMP WITH TIME ZONE NOT NULL
);

CREATE TABLE car (
    id SERIAL PRIMARY KEY,
    event_id INT REFERENCES event(id) ON DELETE CASCADE,
    driver VARCHAR(255) NOT NULL,
    max_capacity INT NOT NULL,
    departure_time TIMESTAMP WITH TIME ZONE NOT NULL,
    return_time TIMESTAMP WITH TIME ZONE NOT NULL
);

CREATE TABLE rider (
    id SERIAL PRIMARY KEY,
    car_id INT REFERENCES car(id) ON DELETE CASCADE,
    rider VARCHAR(255) NOT NULL
);
